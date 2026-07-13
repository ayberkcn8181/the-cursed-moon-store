use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde_yaml::{Mapping, Value};

use crate::model::{CompatibilityTool, Game, LauncherId, LauncherInstallation, ToolKind};
use crate::safety::{atomic_write_with_backup, ensure_safe_child, is_process_running};

pub fn installed_tools(installation: &LauncherInstallation) -> Result<Vec<CompatibilityTool>> {
    let mut tools = Vec::new();
    if !installation.tool_root.is_dir() {
        return Ok(tools);
    }
    for entry in fs::read_dir(&installation.tool_root)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_dir() || !path.join("bin/wine").is_file() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().into_owned();
        tools.push(CompatibilityTool {
            name: id.clone(),
            version: id.clone(),
            id,
            path,
            launcher: LauncherId::Lutris,
            kind: ToolKind::Wine,
        });
    }
    tools.sort_by_key(|tool| tool.name.to_lowercase());
    Ok(tools)
}

pub fn games(installation: &LauncherInstallation) -> Result<Vec<Game>> {
    let games_root = installation.config_root.join("games");
    if !games_root.is_dir() {
        return Ok(Vec::new());
    }
    let mut games = Vec::new();
    for entry in fs::read_dir(&games_root)? {
        let entry = entry?;
        let path = entry.path();
        let extension = path.extension().and_then(|value| value.to_str());
        if !matches!(extension, Some("yml" | "yaml")) {
            continue;
        }
        match parse_game(&path, installation) {
            Ok(Some(game)) => games.push(game),
            Ok(None) => {}
            Err(error) => {
                tracing::warn!(path = %path.display(), error = %error, "invalid Lutris game config")
            }
        }
    }
    games.sort_by_key(|game| game.name.to_lowercase());
    Ok(games)
}

fn parse_game(path: &Path, installation: &LauncherInstallation) -> Result<Option<Game>> {
    let text = fs::read_to_string(path)?;
    let value: Value = serde_yaml::from_str(&text)?;
    let Some(root) = value.as_mapping() else {
        return Ok(None);
    };
    let runner = string(root, "runner").unwrap_or_default();
    if !runner.is_empty() && runner != "wine" {
        return Ok(None);
    }
    let id = string(root, "game_slug")
        .or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .map(str::to_string)
        })
        .context("Lutris game has no identifier")?;
    let name = string(root, "name").unwrap_or_else(|| id.clone());
    let game = mapping(root, "game");
    let wine = mapping(root, "wine");
    let prefix = game
        .and_then(|mapping| string(mapping, "prefix"))
        .map(expand_home);
    let selected_tool = wine.and_then(|mapping| string(mapping, "version"));
    let dxvk_enabled = wine.and_then(|mapping| boolean(mapping, "dxvk"));
    Ok(Some(Game {
        id,
        name,
        install_dir: None,
        launcher: LauncherId::Lutris,
        installation: installation.kind,
        selected_tool,
        config_path: Some(path.to_path_buf()),
        prefix,
        dxvk_enabled,
        writable: true,
    }))
}

pub fn set_game_tool(
    game: &Game,
    installation: &LauncherInstallation,
    tool: Option<&str>,
) -> Result<()> {
    update_game(game, installation, true, |root| {
        let wine = ensure_mapping(root, "wine")?;
        let key = Value::String("version".into());
        if let Some(tool) = tool {
            wine.insert(key, Value::String(tool.to_string()));
        } else {
            wine.remove(&key);
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
        ensure_mapping(root, "wine")?.insert(Value::String("dxvk".into()), Value::Bool(enabled));
        Ok(())
    })
}

fn update_game(
    game: &Game,
    installation: &LauncherInstallation,
    check_running: bool,
    update: impl FnOnce(&mut Mapping) -> Result<()>,
) -> Result<()> {
    if check_running && is_process_running(&["lutris"]) {
        bail!("Lutris is running; close it before changing game settings");
    }
    let path = game
        .config_path
        .as_deref()
        .context("Lutris game has no configuration path")?;
    ensure_safe_child(&installation.config_root.join("games"), path)?;
    let text = fs::read_to_string(path)?;
    let mut value: Value = serde_yaml::from_str(&text)?;
    let root = value
        .as_mapping_mut()
        .context("unsupported Lutris game schema")?;
    update(root)?;
    let output = serde_yaml::to_string(&value)?;
    atomic_write_with_backup(path, output.as_bytes())?;
    Ok(())
}

fn mapping<'a>(mapping: &'a Mapping, key: &str) -> Option<&'a Mapping> {
    mapping
        .get(Value::String(key.to_string()))
        .and_then(Value::as_mapping)
}

fn string(mapping: &Mapping, key: &str) -> Option<String> {
    mapping
        .get(Value::String(key.to_string()))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn boolean(mapping: &Mapping, key: &str) -> Option<bool> {
    let value = mapping.get(Value::String(key.to_string()))?;
    value.as_bool().or_else(|| {
        value
            .as_str()
            .and_then(|value| match value.to_ascii_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            })
    })
}

fn ensure_mapping<'a>(root: &'a mut Mapping, key: &str) -> Result<&'a mut Mapping> {
    let key = Value::String(key.to_string());
    if !root.contains_key(&key) {
        root.insert(key.clone(), Value::Mapping(Mapping::new()));
    }
    root.get_mut(&key)
        .and_then(Value::as_mapping_mut)
        .context("Lutris configuration section is not a map")
}

fn expand_home(value: String) -> std::path::PathBuf {
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
    fn parses_lutris_game_and_preserves_runner_settings() {
        let root = std::env::temp_dir().join(format!("tcms-lutris-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let path = root.join("config/games/portal-2.yml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            "name: Portal 2\ngame_slug: portal-2\nrunner: wine\ngame:\n  prefix: /tmp/prefix\nwine:\n  version: wine-ge-8-26\n  dxvk: true\n  esync: true\n",
        )
        .unwrap();
        let installation = LauncherInstallation {
            launcher: LauncherId::Lutris,
            kind: InstallationKind::Native,
            root: root.clone(),
            config_root: root.join("config"),
            tool_root: root.join("runners/wine"),
        };
        let game = parse_game(&path, &installation).unwrap().unwrap();
        assert_eq!(game.name, "Portal 2");
        assert_eq!(game.selected_tool.as_deref(), Some("wine-ge-8-26"));
        assert_eq!(game.dxvk_enabled, Some(true));
        update_game(&game, &installation, false, |root| {
            ensure_mapping(root, "wine")?.insert(Value::String("dxvk".into()), Value::Bool(false));
            Ok(())
        })
        .unwrap();
        let updated: Value = serde_yaml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            updated["wine"]["esync"].as_bool(),
            Some(true),
            "unknown settings must survive"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
