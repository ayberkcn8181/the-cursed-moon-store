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
        cfg.resolved()
    }

    pub fn resolved(mut self) -> Result<Self> {
        let overlay = std::mem::take(&mut self.advanced.raw_overlay);
        if !overlay.trim().is_empty() {
            let mut value =
                toml::Value::try_from(&self).map_err(|e| Error::Config(e.to_string()))?;
            let patch = if overlay.trim_start().starts_with('{') {
                let json: serde_json::Value =
                    serde_json::from_str(&overlay).map_err(|e| Error::Config(e.to_string()))?;
                toml::Value::try_from(json).map_err(|e| Error::Config(e.to_string()))?
            } else {
                toml::from_str::<toml::Value>(&overlay).map_err(|e| Error::Config(e.to_string()))?
            };
            merge_overlay(&mut value, patch, "")?;
            self = value.try_into().map_err(|e| Error::Config(e.to_string()))?;
            if !self.advanced.raw_overlay.trim().is_empty() {
                return Err(Error::Config("nested raw_overlay is not allowed".into()));
            }
        }
        self.validate()?;
        Ok(self)
    }
    fn validate(&self) -> Result<()> {
        let a = &self.advanced;
        if !matches!(a.flatpak_installation.as_str(), "user" | "system") {
            return Err(Error::Config(
                "Flatpak installation must be user or system".into(),
            ));
        }
        let rpc = reqwest::Url::parse(&a.aur_rpc_url)
            .map_err(|e| Error::Config(format!("AUR RPC URL: {e}")))?;
        if !matches!(rpc.scheme(), "http" | "https") || rpc.host_str().is_none() {
            return Err(Error::Config("AUR RPC requires an HTTP(S) URL".into()));
        }
        for line in a
            .flatpak_remotes
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
        {
            let (name, url) = line
                .split_once('|')
                .ok_or_else(|| Error::Config("Flatpak remotes require name|URL".into()))?;
            if !crate::is_safe_pkg_token(name.trim()) {
                return Err(Error::Config("invalid Flatpak remote name".into()));
            }
            let url = reqwest::Url::parse(url.trim()).map_err(|e| Error::Config(e.to_string()))?;
            if !matches!(url.scheme(), "http" | "https" | "file") {
                return Err(Error::Config(
                    "unsupported Flatpak remote URL scheme".into(),
                ));
            }
        }
        for (name, args) in [("pacman", &a.pacman_extra_args), ("AUR", &a.aur_extra_args)] {
            if args
                .split_whitespace()
                .any(|a| !matches!(a, "--noconfirm" | "--needed"))
            {
                return Err(Error::Config(format!(
                    "{name}: only --noconfirm and --needed can be previewed safely"
                )));
            }
        }
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::config_path()?)
    }
    pub fn load_from(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)?;
        let cfg: Self =
            toml::from_str(&text).map_err(|e| Error::Config(format!("invalid config: {e}")))?;
        cfg.resolved()
    }
    pub fn save_to(&self, path: &Path) -> Result<()> {
        let cfg = self.clone().resolved()?;
        let text = toml::to_string_pretty(&cfg)
            .map_err(|e| Error::Config(format!("serialize config: {e}")))?;
        crate::atomic_file::write(path, text.as_bytes())?;
        Ok(())
    }
}
fn merge_overlay(base: &mut toml::Value, patch: toml::Value, prefix: &str) -> Result<()> {
    match (base, patch) {
        (toml::Value::Table(base), toml::Value::Table(patch)) => {
            for (key, value) in patch {
                let path = format!("{prefix}{key}");
                let target = base
                    .get_mut(&key)
                    .ok_or_else(|| Error::Config(format!("unknown overlay key: {path}")))?;
                merge_overlay(target, value, &format!("{path}."))?;
            }
        }
        (base, patch) => *base = patch,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{InstallState, Package, PackageId, PackageSource};

    #[test]
    fn overlays_apply_once_and_unknown_keys_are_rejected() {
        for text in [
            "enable_aur = false\n[advanced]\nflatpak_installation = 'user'",
            r#"{"enable_aur":false,"advanced":{"flatpak_installation":"user"}}"#,
        ] {
            let mut cfg = AppConfig::default();
            cfg.advanced.raw_overlay = text.into();
            let r = cfg.resolved().unwrap();
            assert!(!r.enable_aur);
            assert_eq!(r.advanced.flatpak_installation, "user");
            assert!(r.advanced.raw_overlay.is_empty());
        }
        let mut cfg = AppConfig::default();
        cfg.advanced.raw_overlay = "enable_aru = false".into();
        assert!(cfg.resolved().is_err());
    }
    #[test]
    fn invalid_settings_do_not_truncate_previous_file() {
        let p = std::env::temp_dir().join(format!("tcms-config-{}.toml", std::process::id()));
        let mut cfg = AppConfig::default();
        cfg.save_to(&p).unwrap();
        let before = std::fs::read(&p).unwrap();
        cfg.advanced.flatpak_installation = "invalid".into();
        assert!(cfg.save_to(&p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), before);
        std::fs::remove_file(p).unwrap();
    }
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
            desktop_id: None,
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
            foreign_status: None,
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
