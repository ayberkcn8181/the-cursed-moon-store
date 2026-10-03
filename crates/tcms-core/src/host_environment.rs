//! Restore the desktop session's environment when leaving the AppImage.
use std::ffi::{OsStr, OsString};

const KEYS: &str = include_str!("../../../packaging/appimage/environment.keys");

/// Overrides shared by subprocesses and GIO application launch contexts.
/// None means remove the variable; Some("") deliberately preserves an empty value.
pub fn overrides() -> Vec<(OsString, Option<OsString>)> {
    overrides_from(|key| std::env::var_os(key))
}

fn overrides_from(get: impl Fn(&str) -> Option<OsString>) -> Vec<(OsString, Option<OsString>)> {
    if get("TCMS_APPIMAGE").as_deref() != Some(OsStr::new("1")) {
        return Vec::new();
    }
    let mut result = Vec::new();
    for key in KEYS.lines() {
        let saved = format!("TCMS_HOST_{key}");
        result.push((key.into(), get(&saved)));
        result.push((saved.into(), None));
    }
    for key in ["TCMS_APPIMAGE", "APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
        result.push((key.into(), None));
    }
    result
}

pub fn apply(command: &mut std::process::Command) {
    for (key, value) in overrides() {
        match value {
            Some(value) => command.env(key, value),
            None => command.env_remove(key),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_launch_is_unchanged() {
        assert!(overrides_from(|_| None).is_empty());
    }

    #[test]
    fn restores_values_without_losing_empty_or_unset_state() {
        let changes = overrides_from(|key| match key {
            "TCMS_APPIMAGE" => Some("1".into()),
            "TCMS_HOST_XDG_DATA_DIRS" => Some("/custom data:/usr/share".into()),
            "TCMS_HOST_GTK_PATH" => Some("".into()),
            _ => None,
        });
        let changes: std::collections::HashMap<_, _> = changes.into_iter().collect();
        assert_eq!(changes[OsStr::new("LD_LIBRARY_PATH")], None);
        assert_eq!(changes[OsStr::new("GTK_PATH")], Some("".into()));
        assert_eq!(
            changes[OsStr::new("XDG_DATA_DIRS")],
            Some("/custom data:/usr/share".into())
        );
        assert_eq!(changes[OsStr::new("TCMS_HOST_XDG_DATA_DIRS")], None);
        assert_eq!(changes[OsStr::new("APPIMAGE")], None);
        assert!(!changes.contains_key(OsStr::new("PATH")));
        assert!(!changes.contains_key(OsStr::new("WAYLAND_DISPLAY")));
    }
}
