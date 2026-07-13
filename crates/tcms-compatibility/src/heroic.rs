use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

use crate::model::{CompatibilityTool, Game, LauncherId, LauncherInstallation, ToolKind};
use crate::safety::{atomic_write_with_backup, ensure_safe_child, is_process_running};

pub fn installed_tools(installation: &LauncherInstallation) -> Result<Vec<CompatibilityTool>> {
    let mut tools = Vec::new();
    for (subdirectory, kind) in [("wine", ToolKind::Wine), ("proton", ToolKind::Proton)] {
        let root = installation.tool_root.join(subdirectory);
        if !root.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            let executable = match kind {
                ToolKind::Wine => path.join("bin/wine"),
                ToolKind::Proton => path.join("proton"),
                ToolKind::Dxvk => continue,
            };
            if !path.is_dir() || !executable.is_file() {
                continue;
            }
            let id = entry.file_name().to_string_lossy().into_owned();
            tools.push(CompatibilityTool {
                name: id.clone(),
                version: id.clone(),
                id,
                path,
                launcher: LauncherId::Heroic,
                kind,
            });
        }
    }
    tools.sort_by_key(|tool| tool.name.to_lowercase());
    Ok(tools)
}

pub fn games(installation: &LauncherInstallation) -> Result<Vec<Game>> {
    let Some(root) = games_config_root(installation) else {
        return Ok(Vec::new());
    };
    let mut games = Vec::new();
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        match parse_game(&path, installation) {
            Ok(Some(game)) => games.push(game),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "invalid Heroic game config")
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    Ok(games)
}

fn games_config_root(installation: &LauncherInstallation) -> Option<PathBuf> {
    ["GameConfig", "GamesConfig", "gamesConfig"]
        .into_iter()
        .map(|name| installation.config_root.join(name))
        .find(|path| path.is_dir())
}

fn parse_game(path: &Path, installation: &LauncherInstallation) -> Result<Option<Game>> {
    let text = fs::read_to_string(path)?;
    let value: Value = serde_json::from_str(&text)?;
    let Some(root) = value.as_object() else {
        return Ok(None);
    };
    let id = first_string(root, &["appName", "app_name", "id"])
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .context("Heroic game has no identifier")?;
    let name = first_string(root, &["title", "name"]).unwrap_or_else(|| id.clone());
    let install_dir = first_string(root, &["install_path", "installPath"]).map(expand_home);
    let prefix = first_string(root, &["winePrefix", "wine_prefix"]).map(expand_home);
    let selected_tool = root.get("wineVersion").and_then(selected_tool_id);
    let dxvk_enabled = first_bool(
        root,
        &[
            "autoInstallDxvk",
            "enableDXVK",
            "useDXVK",
            "useDxvk",
            "dxvk",
        ],
    );
    let writable = root.contains_key("wineVersion") && root.contains_key("winePrefix");
    Ok(Some(Game {
        id,
        name,
        install_dir,
        launcher: LauncherId::Heroic,
        installation: installation.kind,
        selected_tool,
        config_path: Some(path.to_path_buf()),
        prefix,
        dxvk_enabled,
        writable,
    }))
}

pub fn set_game_tool(
    game: &Game,
    installation: &LauncherInstallation,
    tool: Option<&CompatibilityTool>,
) -> Result<()> {
    update_game(game, installation, true, |root| {
        if let Some(tool) = tool {
            if tool.launcher != LauncherId::Heroic {
                bail!("compatibility tool belongs to a different launcher");
            }
            let executable = match tool.kind {
                ToolKind::Wine => tool.path.join("bin/wine"),
                ToolKind::Proton => tool.path.join("proton"),
                ToolKind::Dxvk => bail!("DXVK is not a Wine/Proton runner"),
            };
            let mut version = root
                .get("wineVersion")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            version.insert(
                "bin".into(),
                Value::String(executable.to_string_lossy().into_owned()),
            );
            version.insert(
                "name".into(),
                Value::String(format!(
                    "{} - {}",
                    match tool.kind {
                        ToolKind::Wine => "Wine",
                        ToolKind::Proton => "Proton",
                        ToolKind::Dxvk => unreachable!(),
                    },
                    tool.id
                )),
            );
            version.insert(
                "type".into(),
                Value::String(
                    match tool.kind {
                        ToolKind::Wine => "wine",
                        ToolKind::Proton => "proton",
                        ToolKind::Dxvk => unreachable!(),
                    }
                    .into(),
                ),
            );
            if tool.kind == ToolKind::Wine {
                for (key, path) in [
                    ("lib", tool.path.join("lib64")),
                    ("lib32", tool.path.join("lib")),
                    ("wineserver", tool.path.join("bin/wineserver")),
                    ("wineboot", tool.path.join("bin/wineboot")),
                ] {
                    version.insert(
                        key.into(),
                        Value::String(path.to_string_lossy().into_owned()),
                    );
                }
            }
            root.insert("wineVersion".into(), Value::Object(version));
        } else {
            root.remove("wineVersion");
        }
        Ok(())
    })
}

pub fn set_dxvk_enabled(
    game: &Game,
    installation: &LauncherInstallation,
    enabled: bool,
) -> Result<()> {
    update_game(game, installation, true, |root| {
        let key = [
            "autoInstallDxvk",
            "enableDXVK",
            "useDXVK",
            "useDxvk",
            "dxvk",
        ]
        .into_iter()
        .find(|key| root.contains_key(*key))
        .unwrap_or("autoInstallDxvk");
        root.insert(key.into(), Value::Bool(enabled));
        Ok(())
    })
}

fn update_game(
    game: &Game,
    installation: &LauncherInstallation,
    check_running: bool,
    update: impl FnOnce(&mut Map<String, Value>) -> Result<()>,
) -> Result<()> {
    if !game.writable {
        bail!("Heroic configuration schema is unknown; refusing to modify it");
    }
    if check_running && is_process_running(&["heroic", "Heroic", "heroicgameslauncher"]) {
        bail!("Heroic is running; close it before changing game settings");
    }
    let path = game
        .config_path
        .as_deref()
        .context("Heroic game has no configuration path")?;
    ensure_safe_child(&installation.config_root, path)?;
    let text = fs::read_to_string(path)?;
    let mut value: Value = serde_json::from_str(&text)?;
    let root = value
        .as_object_mut()
        .context("unsupported Heroic game schema")?;
    update(root)?;
    let output = serde_json::to_vec_pretty(&value)?;
    atomic_write_with_backup(path, &output)?;
    Ok(())
}

fn first_string(root: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| root.get(*key).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn first_bool(root: &Map<String, Value>, keys: &[&str]) -> Option<bool> {
    keys.iter()
        .find_map(|key| root.get(*key).and_then(Value::as_bool))
}

fn selected_tool_id(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Object(value) => {
            if let Some(name) = first_string(value, &["name", "version"]) {
                return Some(
                    name.strip_prefix("Wine - ")
                        .or_else(|| name.strip_prefix("Proton - "))
                        .unwrap_or(&name)
                        .to_string(),
                );
            }
            first_string(value, &["bin"]).and_then(|bin| {
                Path::new(&bin)
                    .parent()
                    .and_then(Path::parent)
                    .and_then(Path::file_name)
                    .and_then(|name| name.to_str())
                    .map(str::to_string)
            })
        }
        _ => None,
    }
}

fn expand_home(value: String) -> PathBuf {
    if value == "~" {
        return dirs::home_dir().unwrap_or_else(|| value.into());
    }
    if let Some(relative) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(relative);
        }
    }
    value.into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::InstallationKind;

    #[test]
    fn parses_known_schema_and_keeps_unknown_fields() {
        let root = std::env::temp_dir().join(format!("tcms-heroic-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("GamesConfig/portal.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            r#"{"appName":"portal","title":"Portal","winePrefix":"/tmp/prefix","wineVersion":{"name":"Wine-GE","bin":"/tmp/wine","type":"wine"},"customField":42}"#,
        )
        .unwrap();
        let installation = LauncherInstallation {
            launcher: LauncherId::Heroic,
            kind: InstallationKind::Native,
            root: root.clone(),
            config_root: root.clone(),
            tool_root: root.join("tools"),
        };
        let game = parse_game(&path, &installation).unwrap().unwrap();
        assert!(game.writable);
        update_game(&game, &installation, false, |root| {
            root.insert("autoInstallDxvk".into(), Value::Bool(true));
            Ok(())
        })
        .unwrap();
        let updated: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(updated["customField"], 42);
        assert_eq!(updated["autoInstallDxvk"], true);
        fs::remove_dir_all(root).unwrap();
    }
}
