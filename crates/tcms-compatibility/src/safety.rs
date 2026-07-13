use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};

pub fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

pub fn atomic_write_with_backup(path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let parent = path
        .parent()
        .context("configuration has no parent directory")?;
    fs::create_dir_all(parent)?;
    let backup = path.with_extension(format!(
        "{}.tcms-backup-{}",
        path.extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("config"),
        timestamp()
    ));
    if path.is_file() {
        fs::copy(path, &backup)?;
    }
    let temporary = parent.join(format!(
        ".{}.tcms-part-{}",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("config"),
        std::process::id()
    ));
    fs::write(&temporary, bytes)?;
    fs::rename(&temporary, path)?;
    Ok(backup)
}

pub fn is_process_running(names: &[&str]) -> bool {
    let Ok(entries) = fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        if !name.to_string_lossy().chars().all(|ch| ch.is_ascii_digit()) {
            return false;
        }
        fs::read_to_string(entry.path().join("comm"))
            .ok()
            .map(|command| {
                let command = command.trim();
                names.contains(&command)
            })
            .unwrap_or(false)
    })
}

pub fn ensure_safe_child(root: &Path, child: &Path) -> Result<PathBuf> {
    let root = fs::canonicalize(root).with_context(|| format!("resolve {}", root.display()))?;
    let child = fs::canonicalize(child).with_context(|| format!("resolve {}", child.display()))?;
    if child == root || !child.starts_with(&root) {
        bail!("path {} is outside {}", child.display(), root.display());
    }
    Ok(child)
}

pub fn safe_component(value: &str) -> Result<&str> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || !value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '+' | '-'))
    {
        bail!("unsafe path component");
    }
    Ok(value)
}

pub fn archive_path_is_safe(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub fn user_owned_path(path: &Path) -> Result<PathBuf> {
    let home = dirs::home_dir().context("could not resolve home directory")?;
    let canonical =
        fs::canonicalize(path).with_context(|| format!("resolve {}", path.display()))?;
    let home = fs::canonicalize(home)?;
    if canonical == home || !canonical.starts_with(&home) {
        bail!("path is outside the user home directory");
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_paths_reject_escape_and_absolute_paths() {
        assert!(archive_path_is_safe(Path::new("GE-Proton/files/bin")));
        assert!(!archive_path_is_safe(Path::new("../escape")));
        assert!(!archive_path_is_safe(Path::new("/tmp/escape")));
    }

    #[test]
    fn components_reject_separators() {
        assert!(safe_component("GE-Proton10-10").is_ok());
        assert!(safe_component("../GE-Proton").is_err());
        assert!(safe_component("foo/bar").is_err());
    }
}
