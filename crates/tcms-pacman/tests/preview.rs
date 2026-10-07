use std::os::unix::fs::PermissionsExt;
use tcms_core::{Backend, PackageAction, PackageId, PackageSource};
use tcms_pacman::PacmanBackend;
#[tokio::test]
async fn resolves_dependencies_and_sizes_without_running_a_transaction() {
    let root = std::env::temp_dir().join(format!("tcms-preview-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    for (name, script) in [
        ("checkupdates", "#!/bin/sh\nexit 2\n"),
        (
            "pacman",
            r#"#!/bin/sh
case "$*" in
 *-Sup*) case "$*" in *--dbpath*checkupdates*) ;; *) exit 40;; esac
 printf 'app\t2\t500\nlib\t1\t200\n';;
 *-Rnsp*) printf 'app\t1\t1024\nlib\t1\t2048\n';;
 *-Qi*) printf 'Name : app\nVersion : 1\nInstalled Size : 1 KiB\n\n';;
 *-Si*) printf 'Name : app\nVersion : 2\nDownload Size : 500 B\nInstalled Size : 3 KiB\n\nName : lib\nVersion : 1\nInstalled Size : 2 KiB\n\n';;
 *) exit 41;;
esac
"#,
        ),
    ] {
        let p = root.join(name);
        std::fs::write(&p, script).unwrap();
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old = std::env::var("PATH").unwrap();
    std::env::set_var("PATH", format!("{}:{old}", root.display()));
    let backend = PacmanBackend::default();
    let id = PackageId::new(PackageSource::Pacman, "app");
    let p = backend
        .preview(PackageAction::Install, Some(&id))
        .await
        .unwrap();
    assert_eq!(p.entries.len(), 2);
    assert_eq!(p.download_bytes(), Some(700));
    assert_eq!(p.disk_delta(), Some(4096));
    let p = backend
        .preview(PackageAction::Remove, Some(&id))
        .await
        .unwrap();
    assert_eq!(p.disk_delta(), Some(-3072));
    assert_eq!(p.download_bytes(), Some(0));
    std::env::set_var("PATH", old);
    std::fs::remove_dir_all(root).unwrap();
}
