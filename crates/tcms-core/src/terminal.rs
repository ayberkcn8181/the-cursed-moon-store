//! Interactive helper review: keep the real TTY and require an exit receipt.
use crate::{Error, Result};
use std::{
    io::{Read, Seek, SeekFrom},
    os::unix::fs::DirBuilderExt,
    path::{Path, PathBuf},
};
pub fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
struct Stage(PathBuf);
impl Stage {
    fn new() -> Result<Self> {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tcms-terminal-{}-{now}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        // Never recursively remove files created by the helper or a build script.
        let _ = std::fs::remove_file(self.0.join("output"));
        let _ = std::fs::remove_file(self.0.join("status"));
        let _ = std::fs::remove_dir(&self.0);
    }
}
fn wrapper(program: &str, args: &[String], stage: &Path, recorder: &Path) -> String {
    let command = std::iter::once(program)
        .chain(args.iter().map(String::as_str))
        .map(shell_quote)
        .collect::<Vec<_>>()
        .join(" ");
    format!("umask 077\nSHELL=/bin/sh {} -q -e -f -c {} {}\ntcms_status=$?\nprintf '%s\\n' \"$tcms_status\" > {}\nexit \"$tcms_status\"\n", shell_quote(&recorder.to_string_lossy()), shell_quote(&command), shell_quote(&stage.join("output").to_string_lossy()), shell_quote(&stage.join("status").to_string_lossy()))
}
fn terminal() -> Result<(PathBuf, Vec<String>)> {
    for (name, args) in [
        ("konsole", vec!["--separate", "-e"]),
        ("gnome-terminal", vec!["--wait", "--"]),
        ("xterm", vec!["-e"]),
        ("foot", vec!["--"]),
        ("kitty", vec!["--"]),
        ("alacritty", vec!["-e"]),
    ] {
        let path = crate::resolve_program(name);
        if path.is_absolute() && path.is_file() {
            return Ok((path, args.into_iter().map(str::to_string).collect()));
        }
    }
    Err(Error::Message(crate::t("aur.review_no_terminal")))
}
pub async fn run_review(program: &str, args: &[String]) -> Result<()> {
    let (terminal, mut terminal_args) = terminal()?;
    let recorder = crate::resolve_program("script");
    if !recorder.is_absolute() || !recorder.is_file() {
        return Err(Error::Message(crate::t("aur.review_no_script")));
    }
    let stage = Stage::new()?;
    terminal_args.extend([
        "/bin/sh".into(),
        "-c".into(),
        wrapper(program, args, &stage.0, &recorder),
    ]);
    crate::process::report_progress(&format!("{}\n", crate::t("aur.review_terminal")));
    let terminal_result = crate::run(&terminal, &terminal_args).await;
    if let Ok(mut file) = std::fs::File::open(stage.0.join("output")) {
        let len = file.metadata()?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(65536)))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        crate::process::report_progress(&String::from_utf8_lossy(&bytes));
    }
    match std::fs::read_to_string(stage.0.join("status"))
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
    {
        Some(0) => Ok(()),
        Some(code) => Err(Error::Command(format!(
            "AUR review/build exited with status {code}"
        ))),
        None => {
            if let Ok(out) = terminal_result {
                out.ensure_success("AUR terminal")?;
            }
            Err(Error::Message(crate::t("aur.review_interrupted")))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quoting_preserves_metacharacters() {
        let value = "a'b $(printf unsafe); `printf unsafe`\nlast";
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &format!("printf '%s' {}", shell_quote(value))])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(String::from_utf8(out.stdout).unwrap(), value);
    }
    #[test]
    fn receipt_preserves_failure_and_output() {
        let stage = Stage::new().unwrap();
        let recorder = crate::resolve_program("script");
        let cmd = wrapper(
            "/bin/sh",
            &["-c".into(), "printf reviewed; exit 7".into()],
            &stage.0,
            &recorder,
        );
        let out = std::process::Command::new("/bin/sh")
            .args(["-c", &cmd])
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(
            std::fs::read_to_string(stage.0.join("status"))
                .unwrap()
                .trim(),
            "7"
        );
        assert!(std::fs::read_to_string(stage.0.join("output"))
            .unwrap()
            .contains("reviewed"));
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            stage.0.metadata().unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
