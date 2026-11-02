//! Preview/execution integration using isolated command fixtures.
use std::{fs, os::unix::fs::PermissionsExt};
use tcms_pacman::PacmanBackend;
#[tokio::test]
async fn cleanup_revalidates_packages_and_cache_before_any_privileged_command() {
    let root = std::env::temp_dir().join(format!("tcms-cleanup-{}", std::process::id()));
    let cache = root.join("cache");
    fs::create_dir_all(&cache).unwrap();
    fs::write(cache.join("orphan-1-1-any.pkg.tar.zst"), "archive").unwrap();
    fs::write(root.join("pacman"),r#"#!/bin/sh
case "$1" in
-Qdtq) if [ -f "$TCMS_CLEANUP_FIXTURE/changed" ]; then exit 1; fi; echo orphan;;
-Qi) printf 'Name : orphan\nVersion : 1-1\nInstalled Size : 1.00 MiB\nInstall Reason : Installed as a dependency for another package\n';;
-R) printf '%s\n' "$*" >> "$TCMS_CLEANUP_FIXTURE/writes";;
*) exit 9;;
esac
"#).unwrap();
    fs::write(
        root.join("pacman-conf"),
        "#!/bin/sh\nprintf '%s/cache\\n' \"$TCMS_CLEANUP_FIXTURE\"\n",
    )
    .unwrap();
    fs::write(
        root.join("paccache"),
        r#"#!/bin/sh
case "$1" in
--dryrun) printf '%s/cache/orphan-1-1-any.pkg.tar.zst\0\n' "$TCMS_CLEANUP_FIXTURE";;
--remove) printf '%s\n' "$*" >> "$TCMS_CLEANUP_FIXTURE/writes";;
*) exit 9;;
esac
"#,
    )
    .unwrap();
    fs::write(root.join("pkexec"), "#!/bin/sh\nexec \"$@\"\n").unwrap();
    for n in ["pacman", "pacman-conf", "paccache", "pkexec"] {
        fs::set_permissions(root.join(n), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let old = std::env::var_os("PATH").unwrap();
    std::env::set_var(
        "PATH",
        std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(&old)))
            .unwrap(),
    );
    std::env::set_var("TCMS_CLEANUP_FIXTURE", &root);
    let backend = PacmanBackend::default();
    let orphans = backend.orphans().await.unwrap();
    assert_eq!(orphans.len(), 1);
    fs::write(root.join("changed"), "").unwrap();
    assert!(backend.remove_orphans(&orphans).await.is_err());
    assert!(!root.join("writes").exists());
    fs::remove_file(root.join("changed")).unwrap();
    backend.remove_orphans(&orphans).await.unwrap();
    let plan = backend.cache_plan().await.unwrap();
    assert_eq!(plan.files.len(), 1);
    fs::write(&plan.files[0].path, "changed archive").unwrap();
    assert!(backend.clean_cache(&plan).await.is_err());
    assert_eq!(
        fs::read_to_string(root.join("writes"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let plan = backend.cache_plan().await.unwrap();
    backend.clean_cache(&plan).await.unwrap();
    let writes = fs::read_to_string(root.join("writes")).unwrap();
    assert_eq!(writes.lines().next().unwrap(), "-R --noconfirm -- orphan");
    assert!(writes
        .lines()
        .nth(1)
        .unwrap()
        .starts_with("--remove --keep 3 --cachedir "));
    std::env::set_var("PATH", old);
    std::env::remove_var("TCMS_CLEANUP_FIXTURE");
    fs::remove_dir_all(root).unwrap();
}
