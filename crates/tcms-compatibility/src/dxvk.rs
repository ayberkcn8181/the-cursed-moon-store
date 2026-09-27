use std::collections::HashSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{bail, Context, Result};
use cap_std::fs::{Dir, OpenOptions};
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
static DXVK_TRANSACTIONS: Mutex<()> = Mutex::new(());

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
    let _guard = DXVK_TRANSACTIONS.lock().unwrap_or_else(|e| e.into_inner());
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
    let dir = Dir::open_ambient_dir(&prefix, cap_std::ambient_authority())?;
    if dir.symlink_metadata(STATE_FILE).is_ok() {
        bail!("this prefix already has a TCMS-managed DXVK installation");
    }
    let backup_dir =
        PathBuf::from(".tcms-backups").join(format!("dxvk-{}-{}", timestamp(), std::process::id()));
    no_symlinks(&dir, Path::new(".tcms-backups"))?;
    dir.create_dir_all(".tcms-backups")?;
    dir.create_dir(&backup_dir)?;
    let mut state = DxvkState {
        version: version.into(),
        backup_dir,
        files: Vec::new(),
    };

    let operation = (|| -> Result<()> {
        for (arch, system_dir) in [("x64", "system32"), ("x32", "syswow64")] {
            let target_directory = PathBuf::from("drive_c/windows").join(system_dir);
            no_symlinks(&dir, &target_directory)?;
            dir.create_dir_all(&target_directory)?;
            for dll in DLLS {
                let source = artifact.join(arch).join(dll);
                if !source.is_file() {
                    continue;
                }
                let target = target_directory.join(dll);
                no_symlinks(&dir, &target)?;
                let backup = if dir.is_file(&target) {
                    let backup = state.backup_dir.join(arch).join(dll);
                    dir.create_dir_all(backup.parent().context("backup parent")?)?;
                    let mut original = dir.open(&target)?;
                    write_new(&dir, &backup, &mut original)?;
                    Some(backup)
                } else {
                    None
                };
                let mut input = fs::File::open(&source)?;
                state.files.push(ManagedFile {
                    target: target.clone(),
                    backup,
                });
                replace_file(&dir, &target, &mut input)?;
            }
        }
        if state.files.is_empty() {
            bail!("DXVK artifact contains no supported DLL files");
        }
        save_state(&dir, &state)
    })();
    if let Err(error) = operation {
        if let Err(restore_error) = restore_files(&dir, &state) {
            let state_result = save_state(&dir, &state);
            bail!("DXVK install failed: {error}; rollback failed: {restore_error}; recovery state: {state_result:?}");
        }
        return Err(error);
    }
    Ok(())
}

pub fn rollback(prefix: &Path) -> Result<()> {
    let _guard = DXVK_TRANSACTIONS.lock().unwrap_or_else(|e| e.into_inner());
    rollback_inner(&user_owned_path(prefix)?)
}

fn rollback_inner(prefix: &Path) -> Result<()> {
    let dir = Dir::open_ambient_dir(prefix, cap_std::ambient_authority())?;
    no_symlinks(&dir, Path::new(STATE_FILE))?;
    let state: DxvkState = serde_json::from_slice(
        &dir.read(STATE_FILE)
            .context("this prefix has no TCMS DXVK state")?,
    )?;
    restore_files(&dir, &state)?;
    dir.remove_file(STATE_FILE)?;
    Ok(())
}

/// Validate the entire journal before touching any DLL. The journal is user data,
/// not authority to replace arbitrary files or recursively delete a directory.
fn validate_state(dir: &Dir, state: &DxvkState) -> Result<()> {
    let backup_parts: Vec<_> = state.backup_dir.iter().collect();
    if backup_parts.len() != 2
        || backup_parts[0] != ".tcms-backups"
        || !backup_parts[1].to_str().is_some_and(|name| {
            name.strip_prefix("dxvk-").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit() || c == '-')
            })
        })
    {
        bail!("invalid DXVK backup directory");
    }
    no_symlinks(dir, &state.backup_dir)?;
    let mut seen = HashSet::new();
    for file in &state.files {
        let parts: Vec<_> = file.target.iter().collect();
        if parts.len() != 4
            || parts[0] != "drive_c"
            || parts[1] != "windows"
            || !matches!(parts[2].to_str(), Some("system32" | "syswow64"))
            || !parts[3].to_str().is_some_and(|name| DLLS.contains(&name))
            || !seen.insert(file.target.clone())
        {
            bail!("invalid or duplicate DXVK target");
        }
        no_symlinks(dir, &file.target)?;
        if let Some(backup) = &file.backup {
            let arch = if parts[2] == "system32" { "x64" } else { "x32" };
            if *backup != state.backup_dir.join(arch).join(parts[3]) {
                bail!("DXVK backup does not match its target");
            }
            no_symlinks(dir, backup)?;
            if !dir.is_file(backup) {
                bail!("DXVK backup is missing: {}", backup.display());
            }
        }
    }
    Ok(())
}

fn restore_files(dir: &Dir, state: &DxvkState) -> Result<()> {
    validate_state(dir, state)?;
    for file in state.files.iter().rev() {
        if let Some(backup) = &file.backup {
            let mut original = dir.open(backup)?;
            replace_file(dir, &file.target, &mut original)?;
        } else if dir.is_file(&file.target) {
            dir.remove_file(&file.target)?;
        }
    }
    // Remove only known backup files. Never recursively delete a journal-supplied path.
    for file in &state.files {
        if let Some(backup) = &file.backup {
            dir.remove_file(backup)?;
        }
    }
    for arch in ["x64", "x32"] {
        let path = state.backup_dir.join(arch);
        if dir.is_dir(&path) {
            dir.remove_dir(path)?;
        }
    }
    if dir.is_dir(&state.backup_dir) {
        dir.remove_dir(&state.backup_dir)?;
    }
    Ok(())
}

fn no_symlinks(dir: &Dir, path: &Path) -> Result<()> {
    if !archive_path_is_safe(path) {
        bail!("unsafe DXVK relative path");
    }
    let mut part = PathBuf::new();
    for component in path.components() {
        part.push(component);
        match dir.symlink_metadata(&part) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("refusing DXVK symlink {}", part.display())
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn write_new(dir: &Dir, path: &Path, input: &mut impl Read) -> Result<()> {
    let mut output = dir.open_with(path, OpenOptions::new().write(true).create_new(true))?;
    std::io::copy(input, &mut output)?;
    output.flush()?;
    output.sync_all()?;
    Ok(())
}

fn replace_file(dir: &Dir, target: &Path, input: &mut impl Read) -> Result<()> {
    no_symlinks(dir, target)?;
    let temporary =
        target.with_extension(format!("tcms-part-{}-{}", std::process::id(), timestamp()));
    // Exclusive creation refuses pre-existing symlinks and hard links. Dir confines
    // every operation to the opened prefix, including during parent-path races.
    write_new(dir, &temporary, input)?;
    if let Err(error) = dir.rename(&temporary, dir, target) {
        let _ = dir.remove_file(temporary);
        return Err(error.into());
    }
    Ok(())
}

fn save_state(dir: &Dir, state: &DxvkState) -> Result<()> {
    replace_file(
        dir,
        Path::new(STATE_FILE),
        &mut serde_json::to_vec_pretty(state)?.as_slice(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("tcms-dxvk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn journal(target: &str) -> DxvkState {
        DxvkState {
            version: "test".into(),
            backup_dir: ".tcms-backups/dxvk-1".into(),
            files: vec![ManagedFile {
                target: target.into(),
                backup: None,
            }],
        }
    }

    #[test]
    fn rollback_rejects_parent_symlink_without_touching_external_file() {
        let root = fixture("parent-symlink");
        let prefix = root.join("prefix");
        let outside = root.join("outside");
        fs::create_dir_all(prefix.join("drive_c/windows")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("dxgi.dll"), b"do not delete").unwrap();
        std::os::unix::fs::symlink(&outside, prefix.join("drive_c/windows/system32")).unwrap();
        let dir = Dir::open_ambient_dir(&prefix, cap_std::ambient_authority()).unwrap();
        assert!(restore_files(&dir, &journal("drive_c/windows/system32/dxgi.dll")).is_err());
        assert_eq!(
            fs::read(outside.join("dxgi.dll")).unwrap(),
            b"do not delete"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_rejects_arbitrary_backup_directory_before_restoring_files() {
        let root = fixture("bad-backup");
        fs::create_dir_all(root.join("drive_c/windows/system32")).unwrap();
        fs::write(root.join("drive_c/windows/system32/dxgi.dll"), b"keep").unwrap();
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let mut state = journal("drive_c/windows/system32/dxgi.dll");
        state.backup_dir = "drive_c".into();
        assert!(restore_files(&dir, &state).is_err());
        assert_eq!(
            fs::read(root.join("drive_c/windows/system32/dxgi.dll")).unwrap(),
            b"keep"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rollback_preflights_all_backups_before_changing_any_dll() {
        let root = fixture("missing-backup");
        fs::create_dir_all(root.join("drive_c/windows/system32")).unwrap();
        let target = root.join("drive_c/windows/system32/dxgi.dll");
        fs::write(&target, b"keep").unwrap();
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        let mut state = journal("drive_c/windows/system32/d3d11.dll");
        state.files[0].backup = Some(".tcms-backups/dxvk-1/x64/d3d11.dll".into());
        state.files.push(ManagedFile {
            target: "drive_c/windows/system32/dxgi.dll".into(),
            backup: None,
        });
        assert!(restore_files(&dir, &state).is_err());
        assert_eq!(fs::read(target).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn exclusive_temp_creation_refuses_existing_links() {
        let root = fixture("temp-symlink");
        let outside = root.join("outside");
        fs::write(&outside, b"keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("temp")).unwrap();
        let dir = Dir::open_ambient_dir(&root, cap_std::ambient_authority()).unwrap();
        assert!(write_new(&dir, Path::new("temp"), &mut b"bad".as_slice()).is_err());
        assert_eq!(fs::read(outside).unwrap(), b"keep");
        fs::remove_dir_all(root).unwrap();
    }

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
