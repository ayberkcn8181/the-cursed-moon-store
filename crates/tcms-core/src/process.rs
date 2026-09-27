use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::process::Stdio;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

tokio::task_local! {
    static PROGRESS: std::sync::mpsc::SyncSender<String>;
}

/// Only explicitly scoped transactions stream output; catalog queries do not.
pub async fn with_progress<T>(
    sender: std::sync::mpsc::SyncSender<String>,
    operation: impl std::future::Future<Output = T>,
) -> T {
    PROGRESS.scope(sender, operation).await
}

pub fn report_progress(message: &str) {
    let _ = PROGRESS.try_with(|sender| sender.try_send(message.to_string()));
}

async fn execute(cmd: &mut Command) -> std::io::Result<std::process::Output> {
    let Ok(sender) = PROGRESS.try_with(Clone::clone) else {
        return cmd.output().await;
    };
    let mut child = cmd.spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (stdout, stderr, status) = tokio::try_join!(
        stream_output(stdout, sender.clone()),
        stream_output(stderr, sender),
        child.wait()
    )?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

async fn stream_output(
    mut pipe: impl AsyncRead + Unpin,
    sender: std::sync::mpsc::SyncSender<String>,
) -> std::io::Result<Vec<u8>> {
    const MAX_RETAINED: usize = 128 * 1024;
    let mut retained = Vec::new();
    let mut buffer = [0_u8; 4096];
    let mut escape = false;
    let mut csi = false;
    loop {
        let count = pipe.read(&mut buffer).await?;
        if count == 0 {
            break;
        }
        retained.extend_from_slice(&buffer[..count]);
        if retained.len() > MAX_RETAINED {
            retained.drain(..retained.len() - MAX_RETAINED);
        }
        let mut clean = Vec::with_capacity(count);
        for &byte in &buffer[..count] {
            if escape {
                if !csi && byte == b'[' {
                    csi = true;
                    continue;
                }
                if !csi || (0x40..=0x7e).contains(&byte) {
                    escape = false;
                    csi = false;
                }
                continue;
            }
            match byte {
                0x1b => escape = true,
                b'\r' => clean.push(b'\n'),
                b'\n' | b'\t' | 0x20..=0xff => clean.push(byte),
                _ => {}
            }
        }
        if !clean.is_empty() {
            let _ = sender.try_send(String::from_utf8_lossy(&clean).into_owned());
        }
    }
    Ok(retained)
}

use crate::error::{Error, Result};

#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }

    pub fn ensure_success(&self, context: &str) -> Result<()> {
        if self.success() {
            Ok(())
        } else {
            let detail = if self.stderr.trim().is_empty() {
                self.stdout.trim().to_string()
            } else {
                self.stderr.trim().to_string()
            };
            let detail = if detail.is_empty() {
                "no output (authentication cancelled or command failed silently)".into()
            } else {
                detail
            };
            Err(Error::Command(format!(
                "{context} failed (exit {}): {detail}",
                self.status
            )))
        }
    }
}

/// Resolve `program` to an absolute path when possible (required by pkexec).
pub fn resolve_program(program: &str) -> PathBuf {
    if program.starts_with('/') {
        return PathBuf::from(program);
    }
    // Common absolute locations first.
    for candidate in [
        format!("/usr/bin/{program}"),
        format!("/usr/local/bin/{program}"),
        format!("/bin/{program}"),
    ] {
        if std::path::Path::new(&candidate).is_file() {
            return PathBuf::from(candidate);
        }
    }
    if let Ok(path) = which_sync(program) {
        return path;
    }
    PathBuf::from(program)
}

fn which_sync(program: &str) -> std::result::Result<PathBuf, ()> {
    let output = std::process::Command::new("which")
        .arg(program)
        .output()
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if path.is_empty() {
        Err(())
    } else {
        Ok(PathBuf::from(path))
    }
}

fn inherit_gui_env(cmd: &mut Command) {
    // Polkit agents need the calling session's display/DBus environment.
    // Do not override LANG/LC_* here — callers set those explicitly when needed.
    for key in [
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
        "DESKTOP_SESSION",
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_TYPE",
        "HOME",
        "USER",
        "LOGNAME",
    ] {
        if let Ok(val) = std::env::var(key) {
            cmd.env(key, val);
        }
    }
}

/// Run a command with `LANG=C` for stable machine-readable output.
pub async fn run<S, I>(program: S, args: I) -> Result<CommandOutput>
where
    S: AsRef<OsStr>,
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    run_with_env(program, args, &[]).await
}

/// Additional environment is supplied by trusted backend code, never shell text.
pub async fn run_with_env<S, I>(
    program: S,
    args: I,
    env: &[(&str, &OsStr)],
) -> Result<CommandOutput>
where
    S: AsRef<OsStr>,
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    let mut cmd = Command::new(program.as_ref());
    cmd.args(args)
        .env("LANG", "C")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    inherit_gui_env(&mut cmd);
    cmd.envs(env.iter().copied());

    let output = execute(&mut cmd)
        .await
        .map_err(|e| Error::Command(format!("failed to spawn {:?}: {e}", program.as_ref())))?;

    Ok(CommandOutput {
        status: output.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

pub async fn run_checked<S, I>(program: S, args: I, context: &str) -> Result<CommandOutput>
where
    S: AsRef<OsStr>,
    I: IntoIterator,
    I::Item: AsRef<OsStr>,
{
    let out = run(program, args).await?;
    out.ensure_success(context)?;
    Ok(out)
}

/// Prefer `pkexec` when available so GUI installs can elevate.
pub async fn run_privileged_pacman(args: &[&str], context: &str) -> Result<CommandOutput> {
    run_privileged("pacman", args, context).await
}

/// Absolute path to pkexec when available.
pub fn pkexec_path() -> Option<PathBuf> {
    let path = resolve_program("pkexec");
    path.is_file().then_some(path)
}

/// Run a privileged command via `pkexec` when available.
///
/// Uses an absolute program path (pkexec requirement) and preserves the
/// graphical session environment so the Polkit auth dialog can appear.
pub async fn run_privileged(program: &str, args: &[&str], context: &str) -> Result<CommandOutput> {
    let program_path = resolve_program(program);
    let program_os: OsString = program_path.as_os_str().to_os_string();

    if let Some(pkexec) = pkexec_path() {
        let mut cmd = Command::new(&pkexec);
        cmd.arg(&program_os).args(args);
        inherit_gui_env(&mut cmd);
        // Keep user's locale for Polkit dialog text; only force C for pacman parsers elsewhere.
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let output = execute(&mut cmd)
            .await
            .map_err(|e| Error::Command(format!("failed to spawn pkexec for {program}: {e}")))?;

        let out = CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        };
        if !out.success() {
            let hint = if out.status == 126 || out.status == 127 {
                " (is the Polkit agent running? try unlocking the screen)"
            } else if out.stderr.to_lowercase().contains("not authorized") || out.status == 126 {
                " (authorization denied or cancelled)"
            } else {
                ""
            };
            let detail = if out.stderr.trim().is_empty() {
                out.stdout.trim().to_string()
            } else {
                out.stderr.trim().to_string()
            };
            return Err(Error::Command(format!(
                "{context} failed via pkexec (exit {}){hint}: {detail}",
                out.status
            )));
        }
        Ok(out)
    } else {
        // Fall back to direct execution (may fail without root).
        let str_args: Vec<&str> = args.to_vec();
        let out = run(&program_os, &str_args).await?;
        out.ensure_success(context)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn transaction_output_streams_both_pipes_and_keeps_exit_status() {
        let (tx, rx) = std::sync::mpsc::sync_channel(16);
        let out = with_progress(
            tx,
            run(
                "sh",
                ["-c", "printf 'working\\r'; printf 'failure\\n' >&2; exit 7"],
            ),
        )
        .await
        .unwrap();
        assert_eq!(out.status, 7);
        assert!(out.stderr.contains("failure"));
        let streamed: String = rx.try_iter().collect();
        assert!(streamed.contains("working"));
        assert!(streamed.contains("failure"));
        assert!(!streamed.contains('\r'));
    }

    #[test]
    fn resolve_pacman_absolute() {
        let path = resolve_program("pacman");
        assert!(
            path.to_string_lossy().contains("pacman"),
            "unexpected path: {path:?}"
        );
    }
}
