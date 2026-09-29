use std::process::Command;

#[test]
fn subprocess_restores_host_environment() {
    // Re-exec the test in its own environment; no process-global mutations that
    // could race other tests or Tokio workers.
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "host_command_child", "--nocapture"])
        .env("TCMS_APPIMAGE", "1")
        .env("APPDIR", "/bundle")
        .env("LD_LIBRARY_PATH", "/bundle/lib")
        .env_remove("TCMS_HOST_LD_LIBRARY_PATH")
        .env("XDG_DATA_DIRS", "/bundle/share")
        .env("TCMS_HOST_XDG_DATA_DIRS", "/custom data:/usr/share")
        .env("GTK_PATH", "/bundle/gtk")
        .env("TCMS_HOST_GTK_PATH", "")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
}

#[tokio::test]
async fn host_command_child() {
    if std::env::var("TCMS_APPIMAGE").as_deref() != Ok("1") {
        return;
    }
    let output = tcms_core::run("/usr/bin/env", std::iter::empty::<&str>())
        .await
        .unwrap();
    assert!(output.success());
    let lines: Vec<_> = output.stdout.lines().collect();
    assert!(lines.contains(&"XDG_DATA_DIRS=/custom data:/usr/share"));
    assert!(lines.contains(&"GTK_PATH="));
    assert!(!lines
        .iter()
        .any(|line| line.starts_with("LD_LIBRARY_PATH=")));
    assert!(!lines.iter().any(|line| line.starts_with("TCMS_")));
    assert!(!lines.iter().any(|line| line.starts_with("APPDIR=")));
}
