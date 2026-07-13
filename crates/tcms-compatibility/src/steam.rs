use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::model::{
    CompatibilityTool, Game, InstallationKind, LauncherId, LauncherInstallation, ToolKind,
};
use crate::safety::{atomic_write_with_backup, is_process_running};
use crate::vdf::{self, Value};

pub fn detect_installations() -> Vec<LauncherInstallation> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    detect_installations_in(&home)
}

pub fn detect_installations_in(home: &Path) -> Vec<LauncherInstallation> {
    let candidates = [
        (InstallationKind::Native, home.join(".local/share/Steam")),
        (InstallationKind::Native, home.join(".steam/steam")),
        (
            InstallationKind::Flatpak,
            home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"),
        ),
    ];
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .filter(|(_, root)| root.is_dir())
        .filter_map(|(kind, root)| {
            let identity = fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            if !seen.insert((kind, identity)) {
                return None;
            }
            Some(LauncherInstallation {
                launcher: LauncherId::Steam,
                kind,
                config_root: root.join("config"),
                tool_root: root.join("compatibilitytools.d"),
                root,
            })
        })
        .collect()
}

pub fn installed_tools(installation: &LauncherInstallation) -> Result<Vec<CompatibilityTool>> {
    let mut tools = Vec::new();
    let mut roots = vec![installation.tool_root.clone()];
    for library in library_roots(installation).unwrap_or_default() {
        roots.push(library.join("steamapps/common"));
    }
    let mut seen = HashSet::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&root).with_context(|| format!("read {}", root.display()))? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let manifest_path = path.join("compatibilitytool.vdf");
            let fallback_name = entry.file_name().to_string_lossy().into_owned();
            let (id, display_name, version) = if manifest_path.is_file() {
                let text = fs::read_to_string(&manifest_path)?;
                let manifest = vdf::parse(&text)
                    .with_context(|| format!("parse {}", manifest_path.display()))?;
                let (id, display_name) = custom_tool_identity(&manifest, &fallback_name)
                    .unwrap_or_else(|| (fallback_name.clone(), fallback_name.clone()));
                let version = version_from_name(&display_name);
                (id, display_name, version)
            } else if path.join("toolmanifest.vdf").is_file() && path.join("proton").is_file() {
                let id = official_tool_id(&fallback_name);
                let version = fs::read_to_string(path.join("version"))
                    .ok()
                    .and_then(|value| value.split_whitespace().nth(1).map(str::to_string))
                    .unwrap_or_else(|| version_from_name(&fallback_name));
                (id, fallback_name, version)
            } else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            tools.push(CompatibilityTool {
                version,
                id,
                name: display_name,
                path,
                launcher: LauncherId::Steam,
                kind: ToolKind::Proton,
            });
        }
    }
    tools.sort_by_key(|tool| tool.name.to_lowercase());
    Ok(tools)
}

fn custom_tool_identity(manifest: &Value, fallback: &str) -> Option<(String, String)> {
    let compatibility_tools = manifest.get("compatibilitytools")?;
    let tools = compatibility_tools
        .get("compat_tools")
        .unwrap_or(compatibility_tools);
    let (id, value) = tools.entries()?.first()?;
    let display = value
        .get("display_name")
        .and_then(Value::text)
        .unwrap_or(fallback);
    Some((id.clone(), display.to_string()))
}

fn official_tool_id(name: &str) -> String {
    let lower = name.to_ascii_lowercase();
    if lower.contains("experimental") {
        return "proton_experimental".into();
    }
    if lower.contains("hotfix") {
        return "proton_hotfix".into();
    }
    let major = lower
        .strip_prefix("proton ")
        .and_then(|version| version.split('.').next())
        .filter(|version| version.chars().all(|ch| ch.is_ascii_digit()));
    major
        .map(|version| format!("proton_{version}"))
        .unwrap_or_else(|| {
            format!(
                "proton_{}",
                lower
                    .chars()
                    .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
                    .collect::<String>()
                    .trim_matches('_')
            )
        })
}

pub fn games(installation: &LauncherInstallation) -> Result<Vec<Game>> {
    let libraries = library_roots(installation)?;
    let overrides = compatibility_overrides(installation).unwrap_or_default();
    let mut games = Vec::new();
    let mut seen = HashSet::new();
    for library in libraries {
        let steamapps = library.join("steamapps");
        let Ok(entries) = fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                continue;
            }
            let Ok(text) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(manifest) = vdf::parse(&text) else {
                continue;
            };
            let Some(state) = manifest.get("AppState") else {
                continue;
            };
            let Some(id) = state.get("appid").and_then(Value::text) else {
                continue;
            };
            if !seen.insert(id.to_string()) {
                continue;
            }
            let name = state
                .get("name")
                .and_then(Value::text)
                .unwrap_or(id)
                .to_string();
            let install_dir = state
                .get("installdir")
                .and_then(Value::text)
                .map(|dir| steamapps.join("common").join(dir));
            games.push(Game {
                id: id.to_string(),
                name,
                install_dir,
                launcher: LauncherId::Steam,
                installation: installation.kind,
                selected_tool: overrides.get(id).cloned(),
                config_path: Some(installation.config_root.join("config.vdf")),
                prefix: Some(steamapps.join("compatdata").join(id).join("pfx")),
                dxvk_enabled: None,
                writable: true,
            });
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    Ok(games)
}

fn library_roots(installation: &LauncherInstallation) -> Result<Vec<PathBuf>> {
    let mut roots = vec![installation.root.clone()];
    let path = installation.root.join("steamapps/libraryfolders.vdf");
    if !path.is_file() {
        return Ok(roots);
    }
    let text = fs::read_to_string(&path)?;
    let parsed = vdf::parse(&text)?;
    if let Some(entries) = parsed.get("libraryfolders").and_then(Value::entries) {
        for (key, value) in entries {
            if !key.chars().all(|ch| ch.is_ascii_digit()) {
                continue;
            }
            let path = value
                .get("path")
                .and_then(Value::text)
                .or_else(|| value.text());
            if let Some(path) = path {
                let path = PathBuf::from(path);
                if path.is_absolute() && !roots.contains(&path) {
                    roots.push(path);
                }
            }
        }
    }
    Ok(roots)
}

fn compatibility_overrides(installation: &LauncherInstallation) -> Result<HashMap<String, String>> {
    let path = installation.config_root.join("config.vdf");
    if !path.is_file() {
        return Ok(HashMap::new());
    }
    let text = fs::read_to_string(path)?;
    let parsed = vdf::parse(&text)?;
    let Some(mapping) = find_object(&parsed, "CompatToolMapping") else {
        return Ok(HashMap::new());
    };
    let mut overrides = HashMap::new();
    for (app_id, value) in mapping.entries().unwrap_or_default() {
        if let Some(name) = value.get("name").and_then(Value::text) {
            if !name.is_empty() {
                overrides.insert(app_id.clone(), name.to_string());
            }
        }
    }
    Ok(overrides)
}

fn find_object<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    if let Some(found) = value.get(key) {
        return Some(found);
    }
    value
        .entries()?
        .iter()
        .find_map(|(_, child)| find_object(child, key))
}

pub fn set_game_tool(
    installation: &LauncherInstallation,
    app_id: &str,
    tool_id: Option<&str>,
) -> Result<PathBuf> {
    if app_id.is_empty() || !app_id.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("invalid Steam app id");
    }
    if steam_is_running(installation.kind) {
        bail!("Steam is running; close it before changing compatibility settings");
    }
    let path = installation.config_root.join("config.vdf");
    set_game_tool_in_file(&path, app_id, tool_id)
}

fn set_game_tool_in_file(path: &Path, app_id: &str, tool_id: Option<&str>) -> Result<PathBuf> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("read Steam configuration {}", path.display()))?;
    let mut root = vdf::parse(&text)?;
    let mapping = root
        .ensure_object("InstallConfigStore")?
        .ensure_object("Software")?
        .ensure_object("Valve")?
        .ensure_object("Steam")?
        .ensure_object("CompatToolMapping")?;
    if let Some(tool_id) = tool_id {
        let mut override_value = Value::object();
        override_value.upsert("name", Value::Text(tool_id.to_string()))?;
        override_value.upsert("config", Value::Text(String::new()))?;
        override_value.upsert("priority", Value::Text("250".to_string()))?;
        mapping.upsert(app_id, override_value)?;
    } else {
        mapping.remove(app_id)?;
    }
    let output = vdf::to_string(&root)?;
    atomic_write_with_backup(path, output.as_bytes())
}

pub fn steam_is_running(kind: InstallationKind) -> bool {
    let _ = kind;
    is_process_running(&["steam", "steamwebhelper"])
}

fn version_from_name(name: &str) -> String {
    name.trim_start_matches("GE-Proton")
        .trim_start_matches("Proton-")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_native_and_flatpak_roots() {
        let root = std::env::temp_dir().join(format!("tcms-steam-detect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".local/share/Steam")).unwrap();
        fs::create_dir_all(root.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"))
            .unwrap();
        let found = detect_installations_in(&root);
        assert_eq!(found.len(), 2);
        assert!(found
            .iter()
            .any(|item| item.kind == InstallationKind::Native));
        assert!(found
            .iter()
            .any(|item| item.kind == InstallationKind::Flatpak));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn updates_and_removes_compat_tool_mapping_with_backup() {
        let root = std::env::temp_dir().join(format!("tcms-steam-vdf-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("config.vdf");
        fs::write(
            &path,
            r#""InstallConfigStore" { "Software" { "Valve" { "Steam" { "CompatToolMapping" { } } } } }"#,
        )
        .unwrap();
        let backup = set_game_tool_in_file(&path, "620", Some("GE-Proton10-10")).unwrap();
        assert!(backup.is_file());
        let parsed = vdf::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        let mapping = find_object(&parsed, "CompatToolMapping").unwrap();
        assert_eq!(
            mapping
                .get("620")
                .and_then(|value| value.get("name"))
                .and_then(Value::text),
            Some("GE-Proton10-10")
        );
        set_game_tool_in_file(&path, "620", None).unwrap();
        let parsed = vdf::parse(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(find_object(&parsed, "CompatToolMapping")
            .unwrap()
            .get("620")
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reads_nested_custom_tool_identity() {
        let manifest = vdf::parse(
            r#""compatibilitytools" {
                "compat_tools" {
                    "GE-Proton10-34" {
                        "install_path" "."
                        "display_name" "GE-Proton10-34"
                    }
                }
            }"#,
        )
        .unwrap();
        assert_eq!(
            custom_tool_identity(&manifest, "fallback"),
            Some(("GE-Proton10-34".into(), "GE-Proton10-34".into()))
        );
    }

    #[test]
    fn derives_official_proton_mapping_ids() {
        assert_eq!(official_tool_id("Proton 11.0"), "proton_11");
        assert_eq!(
            official_tool_id("Proton - Experimental"),
            "proton_experimental"
        );
        assert_eq!(official_tool_id("Proton Hotfix"), "proton_hotfix");
    }

    #[test]
    fn discovers_custom_and_official_proton_tools() {
        let root = std::env::temp_dir().join(format!("tcms-steam-tools-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let custom = root.join("compatibilitytools.d/GE-Proton10-34");
        let official = root.join("steamapps/common/Proton 11.0");
        fs::create_dir_all(&custom).unwrap();
        fs::create_dir_all(&official).unwrap();
        fs::write(
            custom.join("compatibilitytool.vdf"),
            r#""compatibilitytools" { "compat_tools" { "GE-Proton10-34" { "display_name" "GE-Proton10-34" } } }"#,
        )
        .unwrap();
        fs::write(
            official.join("toolmanifest.vdf"),
            r#""manifest" { "commandline" "/proton %verb%" }"#,
        )
        .unwrap();
        fs::write(official.join("proton"), b"#!/bin/sh\n").unwrap();
        fs::write(official.join("version"), "1 proton-11.0-1\n").unwrap();
        let installation = LauncherInstallation {
            launcher: LauncherId::Steam,
            kind: InstallationKind::Native,
            root: root.clone(),
            config_root: root.join("config"),
            tool_root: root.join("compatibilitytools.d"),
        };
        let tools = installed_tools(&installation).unwrap();
        assert!(tools.iter().any(|tool| tool.id == "GE-Proton10-34"));
        assert!(tools.iter().any(|tool| tool.id == "proton_11"));
        fs::remove_dir_all(root).unwrap();
    }
}
