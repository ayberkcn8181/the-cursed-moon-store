use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use crate::package::PackageSource;

/// User-editable repository override (Advanced settings).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoOverride {
    pub name: String,
    pub source: PackageSource,
    pub enabled: bool,
    /// Raw mirror / remote URL or pacman Server line.
    pub url: String,
    /// Optional free-form notes shown in Advanced settings.
    #[serde(default)]
    pub notes: String,
}

/// Advanced knobs — exposed in Settings → Advanced for full manual control.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AdvancedConfig {
    /// Path to pacman.conf (empty = system default).
    pub pacman_conf: String,
    /// Extra pacman args (space-separated), e.g. `--noconfirm`.
    pub pacman_extra_args: String,
    /// Flatpak installation: system | user
    pub flatpak_installation: String,
    /// Flatpak remotes as `name|url` lines.
    pub flatpak_remotes: String,
    /// AUR RPC base URL.
    pub aur_rpc_url: String,
    /// Helper used for AUR builds: paru | yay | makepkg
    pub aur_helper: String,
    /// Extra AUR helper args.
    pub aur_extra_args: String,
    /// Install source priority order (pacman, flatpak, aur).
    pub install_source_priority: Vec<String>,
    /// When true, ask which repository to use on Install.
    pub ask_repo_on_install: bool,
    /// Allow editing raw backend JSON blobs from the UI.
    pub allow_raw_config_edit: bool,
    /// Custom repository overrides list.
    pub repo_overrides: Vec<RepoOverride>,
    /// Raw TOML/JSON snippet merged into runtime (power users).
    pub raw_overlay: String,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            pacman_conf: String::new(),
            pacman_extra_args: String::new(),
            flatpak_installation: "system".into(),
            flatpak_remotes: "flathub|https://dl.flathub.org/repo/flathub.flatpakrepo".into(),
            aur_rpc_url: "https://aur.archlinux.org/rpc".into(),
            aur_helper: "paru".into(),
            aur_extra_args: String::new(),
            install_source_priority: vec!["pacman".into(), "flatpak".into(), "aur".into()],
            ask_repo_on_install: false,
            allow_raw_config_edit: true,
            repo_overrides: Vec::new(),
            raw_overlay: String::new(),
        }
    }
}

impl AdvancedConfig {
    pub fn priority_sources(&self) -> Vec<PackageSource> {
        let mut out = Vec::new();
        for raw in &self.install_source_priority {
            if let Some(src) = PackageSource::from_str_loose(raw) {
                if !out.contains(&src) {
                    out.push(src);
                }
            }
        }
        for src in [
            PackageSource::Pacman,
            PackageSource::Flatpak,
            PackageSource::Aur,
        ] {
            if !out.contains(&src) {
                out.push(src);
            }
        }
        out
    }

    pub fn priority_rank(&self, source: PackageSource) -> usize {
        self.priority_sources()
            .iter()
            .position(|s| *s == source)
            .unwrap_or(99)
    }
}

/// Paths and release preferences for game compatibility tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CompatibilityConfig {
    pub auto_detect: bool,
    pub steam_root: String,
    pub steam_flatpak_root: String,
    pub lutris_root: String,
    pub lutris_flatpak_root: String,
    pub heroic_root: String,
    pub heroic_flatpak_root: String,
    /// `stable` or `prerelease`.
    pub release_channel: String,
    pub allow_artifact_downloads: bool,
}

impl Default for CompatibilityConfig {
    fn default() -> Self {
        Self {
            auto_detect: true,
            steam_root: String::new(),
            steam_flatpak_root: String::new(),
            lutris_root: String::new(),
            lutris_flatpak_root: String::new(),
            heroic_root: String::new(),
            heroic_flatpak_root: String::new(),
            release_channel: "stable".into(),
            allow_artifact_downloads: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub enable_pacman: bool,
    pub enable_flatpak: bool,
    pub enable_aur: bool,
    /// Show codec packages in search and installed lists.
    pub show_codecs: bool,
    /// Show driver / firmware packages in search and installed lists.
    pub show_drivers: bool,
    /// Show system / library / runtime packages in search and installed lists.
    pub show_system_packages: bool,
    pub automatic_updates_check: bool,
    pub download_updates_in_background: bool,
    pub language: String,
    pub compatibility: CompatibilityConfig,
    pub advanced: AdvancedConfig,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            enable_pacman: true,
            enable_flatpak: true,
            enable_aur: true,
            // Consumer-friendly defaults: hide non-app packages.
            show_codecs: false,
            show_drivers: false,
            show_system_packages: false,
            automatic_updates_check: true,
            download_updates_in_background: false,
            language: "system".into(),
            compatibility: CompatibilityConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

impl AppConfig {
    /// Whether a package should appear in Explore search / Installed lists.
    pub fn allows_package(&self, package: &crate::package::Package) -> bool {
        use crate::classify::PackageKind;
        match package.kind() {
            PackageKind::App => true,
            PackageKind::Codec => self.show_codecs,
            PackageKind::Driver => self.show_drivers,
            PackageKind::System => self.show_system_packages,
        }
    }

    pub fn config_dir() -> Result<PathBuf> {
        let base = dirs::config_dir()
            .ok_or_else(|| Error::Config("could not resolve XDG config directory".into()))?;
        Ok(base.join("the-cursed-moon-store"))
    }

    pub fn config_path() -> Result<PathBuf> {
        Ok(Self::config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path)?;
        let cfg: Self = toml::from_str(&text)
            .map_err(|e| Error::Config(format!("invalid config.toml: {e}")))?;
        // Keep "system" as-is; resolve only at runtime via i18n::resolve.
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir()?;
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialize config: {e}")))?;
        fs::write(path, text)?;
        Ok(())
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        toml::from_str(&text).map_err(|e| Error::Config(format!("invalid config: {e}")))
    }

    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("serialize config: {e}")))?;
        fs::write(path, text)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{InstallState, Package, PackageId, PackageSource};

    #[test]
    fn default_priority_prefers_pacman() {
        let adv = AdvancedConfig::default();
        assert_eq!(adv.priority_rank(PackageSource::Pacman), 0);
        assert_eq!(adv.priority_rank(PackageSource::Flatpak), 1);
        assert_eq!(adv.priority_rank(PackageSource::Aur), 2);
    }

    #[test]
    fn custom_priority_order() {
        let adv = AdvancedConfig {
            install_source_priority: vec!["aur".into(), "flatpak".into(), "pacman".into()],
            ..Default::default()
        };
        assert_eq!(adv.priority_rank(PackageSource::Aur), 0);
        assert_eq!(adv.priority_rank(PackageSource::Flatpak), 1);
        assert_eq!(adv.priority_rank(PackageSource::Pacman), 2);
    }

    #[test]
    fn allows_package_respects_visibility() {
        let mut cfg = AppConfig::default();
        let app = Package::stub(
            PackageSource::Pacman,
            "firefox",
            "firefox",
            "Web browser",
            "1",
            InstallState::Available,
        );
        let codec = Package {
            id: PackageId::new(PackageSource::Pacman, "gst-libav"),
            name: "gst-libav".into(),
            summary: "GStreamer codec".into(),
            description: String::new(),
            version: "1".into(),
            available_version: None,
            icon_name: None,
            icon_url: None,
            developer: None,
            publisher: None,
            license: None,
            homepage: None,
            bug_url: None,
            donate_url: None,
            permissions: None,
            is_proprietary: None,
            size_bytes: None,
            state: InstallState::Available,
            installed_elsewhere: false,
            categories: vec!["Codec".into()],
        };
        assert!(cfg.allows_package(&app));
        assert!(!cfg.allows_package(&codec));
        cfg.show_codecs = true;
        assert!(cfg.allows_package(&codec));
    }

    #[test]
    fn roundtrip_toml() {
        let cfg = AppConfig::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let parsed: AppConfig = toml::from_str(&text).unwrap();
        assert_eq!(parsed.enable_pacman, cfg.enable_pacman);
        assert_eq!(
            parsed.advanced.install_source_priority,
            cfg.advanced.install_source_priority
        );
    }
}
