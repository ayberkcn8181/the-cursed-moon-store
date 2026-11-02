//! A failed optional desktop-owner query must not erase the package database.
use std::{fs, os::unix::fs::PermissionsExt};
use tcms_core::{Backend, InstallState};

#[tokio::test]
async fn all_packages_survive_qo_failure_but_q_failure_is_reported() {
    let root = std::env::temp_dir().join(format!("tcms-inventory-{}", std::process::id()));
    fs::create_dir_all(root.join("data/applications")).unwrap();
    fs::write(
        root.join("data/applications/example.desktop"),
        "[Desktop Entry]\nType=Application\nName=Example\nExec=true\n",
    )
    .unwrap();
    let inventory: String = (0..3000)
        .map(|index| format!("package-{index:05} 1.0-1\n"))
        .collect();
    fs::write(root.join("inventory"), inventory).unwrap();
    fs::write(
        root.join("pacman"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$TCMS_INVENTORY_FIXTURE/calls"
if [ "$1" = '--config' ]; then shift 2; fi
case "$1" in
-Q)
  if [ -f "$TCMS_INVENTORY_FIXTURE/fail-query" ]; then
    echo 'local database unavailable' >&2; exit 42
  fi
  cat "$TCMS_INVENTORY_FIXTURE/inventory";;
-Qi)
  if [ -f "$TCMS_INVENTORY_FIXTURE/fail-info" ]; then exit 9; fi
  printf 'Name : package-00000\nInstalled Size : 1.00 GiB\nInstall Reason : Explicitly installed\n\nName : package-00001\nInstalled Size : 2.00 MiB\nInstall Reason : Installed as a dependency for another package\n';;
-Qu) exit 1;;
-Qo) echo 'desktop ownership lookup failed' >&2; exit 2;;
*) exit 9;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(root.join("pacman"), fs::Permissions::from_mode(0o755)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap();
    let old_data = std::env::var_os("XDG_DATA_HOME");
    // This integration binary has a single test and no parallel environment users.
    std::env::set_var(
        "PATH",
        std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(&old_path)))
            .unwrap(),
    );
    std::env::set_var("XDG_DATA_HOME", root.join("data"));
    std::env::set_var("TCMS_INVENTORY_FIXTURE", &root);
    let backend =
        tcms_pacman::PacmanBackend::new(true, root.join("pacman.conf").to_str().unwrap(), "");
    let result = backend.installed().await;
    fs::write(root.join("fail-info"), "").unwrap();
    let without_info = backend.installed().await.unwrap();
    fs::write(root.join("fail-query"), "").unwrap();
    let failed = backend.installed().await;
    std::env::set_var("PATH", old_path);
    match old_data {
        Some(value) => std::env::set_var("XDG_DATA_HOME", value),
        None => std::env::remove_var("XDG_DATA_HOME"),
    }
    std::env::remove_var("TCMS_INVENTORY_FIXTURE");
    let packages = result.unwrap();
    assert_eq!(packages.len(), 3000);
    assert_eq!(packages[0].explicitly_installed, Some(true));
    assert_eq!(packages[1].explicitly_installed, Some(false));
    assert_eq!(packages[2].explicitly_installed, None);
    assert_eq!(packages[0].size_bytes, Some(1024 * 1024 * 1024));
    assert_eq!(without_info.len(), 3000);
    assert!(without_info.iter().all(|p| p.explicitly_installed.is_none() && p.size_bytes.is_none()));
    assert_eq!(packages[0].id.id, "package-00000");
    assert_eq!(packages[2999].id.id, "package-02999");
    assert!(packages
        .iter()
        .all(|package| package.state == InstallState::Installed));
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert_eq!(calls.lines().filter(|l| l.ends_with(" -Qi")).count(), 2);
    assert!(
        calls.lines().any(|line| line.starts_with("-Qo --")),
        "fixture must reproduce the desktop query failure"
    );
    assert!(failed
        .unwrap_err()
        .to_string()
        .contains("local database unavailable"));
    fs::remove_dir_all(root).unwrap();
}
