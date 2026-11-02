//! Test actual argv and stale previews without writing real Flatpak overrides.
use std::{fs, os::unix::fs::PermissionsExt};
use tcms_core::{FlatpakInstallation, FlatpakRef, PackageId, PackageSource};
use tcms_flatpak::{
    permissions::{Change, Permission},
    FlatpakBackend,
};
#[tokio::test]
async fn scoped_permission_changes_reject_stale_overrides_and_reset_only_one_app() {
    let root = std::env::temp_dir().join(format!("tcms-permissions-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("overrides"), "[Context]\nshared=network;\n").unwrap();
    fs::write(
        root.join("flatpak"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$TCMS_PERMISSIONS_FIXTURE/calls"
case "$1" in
info) printf '[Context]\nshared=network;\nsockets=wayland;\n';;
override)
  case " $* " in
    *' --show '*) cat "$TCMS_PERMISSIONS_FIXTURE/overrides";;
    *) printf '%s\n' "$*" >> "$TCMS_PERMISSIONS_FIXTURE/writes";;
  esac;;
*) exit 9;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(root.join("flatpak"), fs::Permissions::from_mode(0o755)).unwrap();
    let old = std::env::var_os("PATH").unwrap();
    std::env::set_var(
        "PATH",
        std::env::join_paths(std::iter::once(root.clone()).chain(std::env::split_paths(&old)))
            .unwrap(),
    );
    std::env::set_var("TCMS_PERMISSIONS_FIXTURE", &root);
    // Exercise the user's installation even when the configured default differs.
    // Never impersonate pkexec: privileged commands deliberately prefer /usr/bin.
    let backend = FlatpakBackend::new(true, "system", "");
    let mut id = PackageId::new(PackageSource::Flatpak, "org.example.App");
    id.flatpak = Some(FlatpakRef {
        kind: "app".into(),
        arch: "x86_64".into(),
        branch: "beta".into(),
        origin: "testing".into(),
        installation: FlatpakInstallation::User,
    });
    let snapshot = backend.permissions(&id).await.unwrap();
    let mut c = Change {
        id,
        expected_overrides: snapshot.overrides,
        changes: vec![(Permission::Network, false)],
        reset: false,
    };
    fs::write(root.join("overrides"), "[Context]\nshared=ipc;\n").unwrap();
    assert_eq!(
        backend
            .change_permissions(&c)
            .await
            .unwrap_err()
            .to_string(),
        tcms_core::t("management.changed")
    );
    assert!(!root.join("writes").exists());
    c.expected_overrides = backend.permissions(&c.id).await.unwrap().overrides;
    backend.change_permissions(&c).await.unwrap();
    c.reset = true;
    c.changes.clear();
    backend.change_permissions(&c).await.unwrap();
    assert_eq!(
        fs::read_to_string(root.join("writes"))
            .unwrap()
            .lines()
            .collect::<Vec<_>>(),
        [
            "override --user --unshare=network -- org.example.App",
            "override --user --reset -- org.example.App"
        ]
    );
    assert!(fs::read_to_string(root.join("calls"))
        .unwrap()
        .contains("info --user --show-permissions -- app/org.example.App/x86_64/beta"));
    std::env::set_var("PATH", old);
    std::env::remove_var("TCMS_PERMISSIONS_FIXTURE");
    fs::remove_dir_all(root).unwrap();
}
