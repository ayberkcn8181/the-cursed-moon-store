use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LauncherId {
    Steam,
    Lutris,
    Heroic,
}

impl fmt::Display for LauncherId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Steam => "Steam",
            Self::Lutris => "Lutris",
            Self::Heroic => "Heroic",
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InstallationKind {
    Native,
    Flatpak,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolKind {
    Proton,
    Wine,
    Dxvk,
}

impl fmt::Display for ToolKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Proton => "Proton",
            Self::Wine => "Wine",
            Self::Dxvk => "DXVK",
        })
    }
}

impl fmt::Display for InstallationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Native => "Native",
            Self::Flatpak => "Flatpak",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherInstallation {
    pub launcher: LauncherId,
    pub kind: InstallationKind,
    pub root: PathBuf,
    pub config_root: PathBuf,
    pub tool_root: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompatibilityTool {
    pub id: String,
    pub name: String,
    pub version: String,
    pub path: PathBuf,
    pub launcher: LauncherId,
    pub kind: ToolKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub install_dir: Option<PathBuf>,
    pub launcher: LauncherId,
    pub installation: InstallationKind,
    pub selected_tool: Option<String>,
    pub config_path: Option<PathBuf>,
    pub prefix: Option<PathBuf>,
    pub dxvk_enabled: Option<bool>,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct GameOverride {
    pub tool_id: Option<String>,
    pub dxvk_enabled: Option<bool>,
}

#[derive(Debug, Clone, Default)]
pub struct CompatibilitySnapshot {
    pub installations: Vec<LauncherInstallation>,
    pub tools: Vec<CompatibilityTool>,
    pub games: Vec<Game>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DiscoveryOptions {
    pub auto_detect: bool,
    pub steam_root: Option<PathBuf>,
    pub steam_flatpak_root: Option<PathBuf>,
    pub lutris_root: Option<PathBuf>,
    pub lutris_flatpak_root: Option<PathBuf>,
    pub heroic_root: Option<PathBuf>,
    pub heroic_flatpak_root: Option<PathBuf>,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            auto_detect: true,
            steam_root: None,
            steam_flatpak_root: None,
            lutris_root: None,
            lutris_flatpak_root: None,
            heroic_root: None,
            heroic_flatpak_root: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ToolRelease {
    pub tag_name: String,
    pub name: Option<String>,
    pub prerelease: bool,
    pub assets: Vec<ReleaseAsset>,
}
