use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::safety::{archive_path_is_safe, timestamp, user_owned_path};

const DLLS: &[&str] = &[
    "d3d8.dll",
    "d3d9.dll",
    "d3d10core.dll",
    "d3d11.dll",
    "dxgi.dll",
];
const STATE_FILE: &str = ".tcms-dxvk.json";

#[derive(Debug, Serialize, Deserialize)]
struct DxvkState {
    version: String,
    backup_dir: PathBuf,
    files: Vec<ManagedFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ManagedFile {
    target: PathBuf,
    backup: Option<PathBuf>,
}

pub fn is_managed(prefix: &Path) -> bool {
    prefix.join(STATE_FILE).is_file()
}

pub fn install(prefix: &Path, artifact: &Path, version: &str) -> Result<()> {
    install_inner(prefix, artifact, version, true)
}

fn install_inner(prefix: &Path, artifact: &Path, version: &str, enforce_home: bool) -> Result<()> {
    if !artifact.join("x64/dxgi.dll").is_file() || !artifact.join("x32/dxgi.dll").is_file() {
        bail!("DXVK artifact is incomplete");
    }
    let prefix = if enforce_home {
        user_owned_path(prefix)?
    } else {
        fs::canonicalize(prefix)?
    };
    if is_managed(&prefix) {
        bail!("this prefix already has a TCMS-managed DXVK installation");
    }

    let backup_dir = prefix
        .join(".tcms-backups")
        .join(format!("dxvk-{}", timestamp()));
    let mut state = DxvkState {
        version: version.to_string(),
        backup_dir: relative_to(&prefix, &backup_dir)?,
        files: Vec::new(),
    };

    let operation = (|| -> Result<()> {
        for (source_arch, target_directory) in [
            ("x64", prefix.join("drive_c/windows/system32")),
            ("x32", prefix.join("drive_c/windows/syswow64")),
        ] {
            fs::create_dir_all(&target_directory)?;
            let target_directory = fs::canonicalize(&target_directory)?;
            if !target_directory.starts_with(&prefix) {
                bail!("Wine system directory escaped the prefix");
            }
            for dll in DLLS {
                let source = artifact.join(source_arch).join(dll);
                if !source.is_file() {
                    continue;
                }
                let target = target_directory.join(dll);
                if target.is_symlink() {
                    bail!("refusing to overwrite symlink {}", target.display());
                }
                let backup = if target.is_file() {
                    let backup = backup_dir.join(source_arch).join(dll);
                    fs::create_dir_all(backup.parent().unwrap_or(&backup_dir))?;
                    fs::copy(&target, &backup)?;
                    Some(relative_to(&prefix, &backup)?)
                } else {
                    None
                };
                let temporary = target.with_extension("dll.tcms-part");
                fs::copy(&source, &temporary)?;
                fs::rename(&temporary, &target)?;
                state.files.push(ManagedFile {
                    target: relative_to(&prefix, &target)?,
                    backup,
                });
            }
        }
        if state.files.is_empty() {
            bail!("DXVK artifact contains no supported DLL files");
        }
        let state_path = prefix.join(STATE_FILE);
        let temporary = prefix.join(format!(".{STATE_FILE}.part"));
        fs::write(&temporary, serde_json::to_vec_pretty(&state)?)?;
        fs::rename(temporary, state_path)?;
        Ok(())
    })();
    if let Err(error) = operation {
        let _ = restore_files(&prefix, &state);
        return Err(error);
    }
    Ok(())
}

pub fn rollback(prefix: &Path) -> Result<()> {
    let prefix = user_owned_path(prefix)?;
    rollback_inner(&prefix)
}

fn rollback_inner(prefix: &Path) -> Result<()> {
    let state_path = prefix.join(STATE_FILE);
    let state: DxvkState = serde_json::from_slice(
        &fs::read(&state_path).context("this prefix has no TCMS DXVK state")?,
    )?;
    restore_files(prefix, &state)?;
    fs::remove_file(state_path)?;
    Ok(())
}

fn restore_files(prefix: &Path, state: &DxvkState) -> Result<()> {
    for file in state.files.iter().rev() {
        validate_relative(&file.target)?;
        let target = prefix.join(&file.target);
        if target.is_symlink() {
            bail!("refusing to modify symlink {}", target.display());
        }
        if let Some(backup) = &file.backup {
            validate_relative(backup)?;
            let backup = prefix.join(backup);
            if !backup.is_file() {
                bail!("DXVK backup is missing: {}", backup.display());
            }
            let temporary = target.with_extension("dll.tcms-restore");
            fs::copy(&backup, &temporary)?;
            fs::rename(temporary, &target)?;
        } else if target.is_file() {
            fs::remove_file(target)?;
        }
    }
    validate_relative(&state.backup_dir)?;
    let backup_dir = prefix.join(&state.backup_dir);
    if backup_dir.is_dir() {
        fs::remove_dir_all(backup_dir)?;
    }
    Ok(())
}

fn relative_to(root: &Path, path: &Path) -> Result<PathBuf> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| anyhow::anyhow!("path escaped Wine prefix"))
}

fn validate_relative(path: &Path) -> Result<()> {
    if !archive_path_is_safe(path) {
        bail!("DXVK state contains an unsafe path");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_and_rollback_restore_original_dlls() {
        let root = std::env::temp_dir().join(format!("tcms-dxvk-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let prefix = root.join("prefix");
        let artifact = root.join("dxvk");
        fs::create_dir_all(prefix.join("drive_c/windows/system32")).unwrap();
        fs::create_dir_all(prefix.join("drive_c/windows/syswow64")).unwrap();
        fs::create_dir_all(artifact.join("x64")).unwrap();
        fs::create_dir_all(artifact.join("x32")).unwrap();
        fs::write(
            prefix.join("drive_c/windows/system32/dxgi.dll"),
            b"original",
        )
        .unwrap();
        fs::write(artifact.join("x64/dxgi.dll"), b"new64").unwrap();
        fs::write(artifact.join("x32/dxgi.dll"), b"new32").unwrap();
        install_inner(&prefix, &artifact, "test", false).unwrap();
        assert_eq!(
            fs::read(prefix.join("drive_c/windows/system32/dxgi.dll")).unwrap(),
            b"new64"
        );
        rollback_inner(&fs::canonicalize(&prefix).unwrap()).unwrap();
        assert_eq!(
            fs::read(prefix.join("drive_c/windows/system32/dxgi.dll")).unwrap(),
            b"original"
        );
        assert!(!prefix.join("drive_c/windows/syswow64/dxgi.dll").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
