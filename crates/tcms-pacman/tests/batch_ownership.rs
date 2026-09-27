//! Run real subprocess plumbing against a controlled pacman fixture.
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

#[tokio::test]
async fn desktop_scan_uses_three_processes_for_257_files() {
    let root = std::env::temp_dir().join(format!("tcms-owners-{}", std::process::id()));
    fs::create_dir_all(root.join("apps")).unwrap();
    let old_path = std::env::var_os("PATH").unwrap();
    let script = root.join("pacman");
    fs::write(
        &script,
        r#"#!/bin/sh
printf 'call\n' >> "$TCMS_FIXTURE_ROOT/calls"
[ "$1" = '-Qo' ] && [ "$2" = '--' ] || exit 9
shift 2
for path do
  printf '%s is owned by tcms-example 1.0-1\n' "$path"
done
"#,
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    for n in 0..257 {
        fs::write(
            root.join(format!("apps/app {n}.desktop")),
            format!("[Desktop Entry]\nName=Example {n}\nType=Application\nExec=true\n"),
        )
        .unwrap();
    }
    // This integration binary contains one test; no parallel environment users.
    std::env::set_var(
        "PATH",
        std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(&old_path)))
            .unwrap(),
    );
    std::env::set_var("TCMS_FIXTURE_ROOT", &root);
    let result =
        tcms_pacman::desktop::discover_desktop_apps_in(&[PathBuf::from(&root).join("apps")]).await;
    std::env::set_var("PATH", old_path);
    std::env::remove_var("TCMS_FIXTURE_ROOT");
    let apps = result.unwrap();
    assert_eq!(apps.len(), 257);
    assert!(apps
        .iter()
        .all(|app| app.package_name.as_deref() == Some("tcms-example")));
    assert_eq!(
        fs::read_to_string(root.join("calls"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    fs::remove_dir_all(root).unwrap();
}
