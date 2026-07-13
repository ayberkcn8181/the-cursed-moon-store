//! Compatibility-tool discovery and management for Linux game launchers.

pub mod dxvk;
pub mod heroic;
pub mod lutris;
mod model;
mod releases;
mod safety;
pub mod steam;
pub mod vdf;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

pub use model::{
    CompatibilitySnapshot, CompatibilityTool, DiscoveryOptions, Game, GameOverride,
    InstallationKind, LauncherId, LauncherInstallation, ReleaseAsset, ToolKind, ToolRelease,
};
pub use releases::{
    dxvk_releases, dxvk_releases_channel, install_dxvk_release, install_proton_ge, install_wine_ge,
    proton_ge_releases, proton_ge_releases_channel, wine_ge_releases, wine_ge_releases_channel,
};

pub fn scan() -> CompatibilitySnapshot {
    scan_with_options(&DiscoveryOptions::default())
}

pub fn scan_with_options(options: &DiscoveryOptions) -> CompatibilitySnapshot {
    let mut snapshot = CompatibilitySnapshot::default();
    let mut installations = Vec::new();
    if options.auto_detect {
        installations.extend(steam::detect_installations());
        installations.extend(detect_other_installations());
    }
    add_overrides(&mut installations, options);
    for installation in &installations {
        let tools = match installation.launcher {
            LauncherId::Steam => steam::installed_tools(installation),
            LauncherId::Lutris => lutris::installed_tools(installation),
            LauncherId::Heroic => heroic::installed_tools(installation),
        };
        match tools {
            Ok(mut tools) => snapshot.tools.append(&mut tools),
            Err(error) => snapshot.warnings.push(error.to_string()),
        }
        let games = match installation.launcher {
            LauncherId::Steam => steam::games(installation),
            LauncherId::Lutris => lutris::games(installation),
            LauncherId::Heroic => heroic::games(installation),
        };
        match games {
            Ok(mut games) => snapshot.games.append(&mut games),
            Err(error) => snapshot.warnings.push(error.to_string()),
        }
    }
    snapshot.installations = installations;
    snapshot
}

fn add_overrides(installations: &mut Vec<LauncherInstallation>, options: &DiscoveryOptions) {
    for (launcher, kind, root) in [
        (
            LauncherId::Steam,
            InstallationKind::Native,
            options.steam_root.as_ref(),
        ),
        (
            LauncherId::Steam,
            InstallationKind::Flatpak,
            options.steam_flatpak_root.as_ref(),
        ),
        (
            LauncherId::Lutris,
            InstallationKind::Native,
            options.lutris_root.as_ref(),
        ),
        (
            LauncherId::Lutris,
            InstallationKind::Flatpak,
            options.lutris_flatpak_root.as_ref(),
        ),
        (
            LauncherId::Heroic,
            InstallationKind::Native,
            options.heroic_root.as_ref(),
        ),
        (
            LauncherId::Heroic,
            InstallationKind::Flatpak,
            options.heroic_flatpak_root.as_ref(),
        ),
    ] {
        let Some(root) = root.filter(|root| root.is_dir()) else {
            continue;
        };
        let item = installation_from_root(launcher, kind, root.clone());
        let identity = fs::canonicalize(&item.root).unwrap_or_else(|_| item.root.clone());
        let duplicate = installations.iter().any(|current| {
            current.launcher == launcher
                && current.kind == kind
                && fs::canonicalize(&current.root).unwrap_or_else(|_| current.root.clone())
                    == identity
        });
        if !duplicate {
            installations.push(item);
        }
    }
}

fn installation_from_root(
    launcher: LauncherId,
    kind: InstallationKind,
    root: PathBuf,
) -> LauncherInstallation {
    match launcher {
        LauncherId::Steam => LauncherInstallation {
            launcher,
            kind,
            config_root: root.join("config"),
            tool_root: root.join("compatibilitytools.d"),
            root,
        },
        LauncherId::Lutris => {
            let config_root = if root.join("games").is_dir() {
                root.clone()
            } else {
                root.join("config")
            };
            LauncherInstallation {
                launcher,
                kind,
                config_root,
                tool_root: root.join("runners/wine"),
                root,
            }
        }
        LauncherId::Heroic => LauncherInstallation {
            launcher,
            kind,
            config_root: root.clone(),
            tool_root: root.join("tools"),
            root,
        },
    }
}

pub fn detect_other_installations() -> Vec<LauncherInstallation> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    detect_other_installations_in(&home)
}

fn detect_other_installations_in(home: &Path) -> Vec<LauncherInstallation> {
    let candidates = [
        (
            LauncherId::Lutris,
            InstallationKind::Native,
            home.join(".local/share/lutris"),
            home.join(".config/lutris"),
            home.join(".local/share/lutris/runners/wine"),
        ),
        (
            LauncherId::Lutris,
            InstallationKind::Flatpak,
            home.join(".var/app/net.lutris.Lutris/data/lutris"),
            home.join(".var/app/net.lutris.Lutris/config/lutris"),
            home.join(".var/app/net.lutris.Lutris/data/lutris/runners/wine"),
        ),
        (
            LauncherId::Heroic,
            InstallationKind::Native,
            home.join(".config/heroic"),
            home.join(".config/heroic"),
            home.join(".config/heroic/tools"),
        ),
        (
            LauncherId::Heroic,
            InstallationKind::Flatpak,
            home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"),
            home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"),
            home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic/tools"),
        ),
    ];
    candidates
        .into_iter()
        .filter(|(_, _, root, config, _)| root.is_dir() || config.is_dir())
        .map(
            |(launcher, kind, root, config_root, tool_root)| LauncherInstallation {
                launcher,
                kind,
                root,
                config_root,
                tool_root,
            },
        )
        .collect()
}

pub fn remove_tool(
    installation: &LauncherInstallation,
    tool: &CompatibilityTool,
    games: &[Game],
) -> Result<()> {
    if games
        .iter()
        .any(|game| game.selected_tool.as_deref() == Some(&tool.id))
    {
        bail!("{} is selected by at least one game", tool.name);
    }
    let root = fs::canonicalize(&installation.tool_root)
        .with_context(|| format!("resolve {}", installation.tool_root.display()))?;
    let path =
        fs::canonicalize(&tool.path).with_context(|| format!("resolve {}", tool.path.display()))?;
    if path.parent() != Some(root.as_path()) {
        bail!("tool path is outside the launcher tool directory");
    }
    fs::remove_dir_all(path)?;
    Ok(())
}

pub fn find_installation(
    installations: &[LauncherInstallation],
    launcher: LauncherId,
    kind: InstallationKind,
) -> Option<&LauncherInstallation> {
    installations
        .iter()
        .find(|item| item.launcher == launcher && item.kind == kind)
}

pub fn display_path(path: &Path) -> String {
    dirs::home_dir()
        .and_then(|home| {
            path.strip_prefix(home)
                .ok()
                .map(|relative| PathBuf::from("~").join(relative))
        })
        .unwrap_or_else(|| path.to_path_buf())
        .display()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_other_launcher_layouts() {
        let root =
            std::env::temp_dir().join(format!("tcms-launcher-detect-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join(".config/lutris")).unwrap();
        fs::create_dir_all(root.join(".var/app/com.heroicgameslauncher.hgl/config/heroic"))
            .unwrap();
        let found = detect_other_installations_in(&root);
        assert_eq!(found.len(), 2);
        assert!(found.iter().any(|item| item.launcher == LauncherId::Lutris));
        assert!(found.iter().any(|item| item.launcher == LauncherId::Heroic));
        fs::remove_dir_all(root).unwrap();
    }
}
