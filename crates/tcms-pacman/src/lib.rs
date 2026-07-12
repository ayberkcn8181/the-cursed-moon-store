//! Pacman (official / system repo) backend.

mod desktop;

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use tcms_core::process::{run, run_privileged_pacman};
use tcms_core::{
    Backend, BackendId, Error, InstallState, Package, PackageId, PackageSource, Result,
    SearchQuery, SearchResult,
};

use crate::desktop::{discover_desktop_apps, DesktopApp};

#[derive(Debug, Clone)]
pub struct PacmanBackend {
    enabled: bool,
    pacman_conf: String,
    extra_args: String,
}

impl PacmanBackend {
    pub fn new(
        enabled: bool,
        pacman_conf: impl Into<String>,
        extra_args: impl Into<String>,
    ) -> Self {
        Self {
            enabled,
            pacman_conf: pacman_conf.into(),
            extra_args: extra_args.into(),
        }
    }

    pub fn pacman_conf(&self) -> &str {
        &self.pacman_conf
    }

    pub fn set_pacman_conf(&mut self, path: impl Into<String>) {
        self.pacman_conf = path.into();
    }

    pub fn extra_args(&self) -> &str {
        &self.extra_args
    }

    pub fn set_extra_args(&mut self, args: impl Into<String>) {
        self.extra_args = args.into();
    }

    fn ensure_enabled(&self) -> Result<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(Error::BackendDisabled("pacman".into()))
        }
    }

    fn conf_args(&self) -> Vec<String> {
        if self.pacman_conf.trim().is_empty() {
            Vec::new()
        } else {
            vec!["--config".into(), self.pacman_conf.clone()]
        }
    }

    fn extra_arg_list(&self) -> Vec<String> {
        self.extra_args
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    async fn query_updates(&self) -> Result<HashMap<String, String>> {
        let mut args = self.conf_args();
        args.push("-Qu".into());
        let out = run("pacman", &args).await?;
        // exit 1 means no updates
        if !out.success() && out.status != 1 {
            out.ensure_success("pacman -Qu")?;
        }
        let mut map = HashMap::new();
        for line in out.stdout.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // name old -> new   OR   name old-version new-version
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 && parts[2] == "->" {
                map.insert(parts[0].to_string(), parts[3].to_string());
            } else if parts.len() >= 3 {
                map.insert(parts[0].to_string(), parts[2].to_string());
            }
        }
        Ok(map)
    }

    fn package_from_desktop(
        app: &DesktopApp,
        updates: &HashMap<String, String>,
    ) -> Option<Package> {
        let pkg_name = app.package_name.as_ref()?;
        let state = if updates.contains_key(pkg_name) {
            InstallState::Updatable
        } else {
            InstallState::Installed
        };
        Some(Package {
            id: PackageId::new(PackageSource::Pacman, pkg_name.clone()),
            name: app.name.clone(),
            summary: app.comment.clone().unwrap_or_else(|| pkg_name.clone()),
            description: app.comment.clone().unwrap_or_default(),
            version: app.version.clone().unwrap_or_else(|| "installed".into()),
            available_version: updates.get(pkg_name).cloned(),
            icon_name: app.icon.clone(),
            icon_url: None,
            publisher: None,
            bug_url: None,
            donate_url: None,
            permissions: None,
            is_proprietary: None,
            developer: None,
            license: None,
            homepage: None,
            size_bytes: None,
            state,
            categories: app.categories.clone(),
        })
    }

    async fn search_repos(&self, text: &str) -> Result<Vec<Package>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        // Avoid pacman -Qu / per-package -Q on every keystroke — trust [installed] markers.
        let mut args = self.conf_args();
        args.push("-Ss".into());
        args.push(text.into());
        let out = run("pacman", &args).await?;
        if !out.success() && out.status != 1 {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();
        let mut lines = out.stdout.lines().peekable();
        while let Some(header) = lines.next() {
            let header = header.trim();
            if header.is_empty() {
                continue;
            }
            // repo/name version [installed]
            let installed = header.contains("[installed]");
            let header_clean = header.replace("[installed]", "");
            let header_clean = header_clean.trim();
            let mut parts = header_clean.split_whitespace();
            let Some(repo_name) = parts.next() else {
                continue;
            };
            let version = parts.next().unwrap_or("").to_string();
            let Some((_, name)) = repo_name.split_once('/') else {
                continue;
            };
            let summary = lines
                .next()
                .map(|l| l.trim().to_string())
                .unwrap_or_default();

            let state = if installed {
                InstallState::Installed
            } else {
                InstallState::Available
            };

            packages.push(Package {
                id: PackageId::new(PackageSource::Pacman, name),
                name: name.to_string(),
                summary: summary.clone(),
                description: summary,
                version: version.clone(),
                available_version: None,
                icon_name: Some("package-x-generic".into()),
                icon_url: None,
                publisher: None,
                bug_url: None,
                donate_url: None,
                permissions: None,
                is_proprietary: None,
                developer: None,
                license: None,
                homepage: None,
                size_bytes: None,
                state,
                categories: Vec::new(),
            });

            if packages.len() >= 60 {
                break;
            }
        }
        Ok(packages)
    }

    async fn query_info(&self, name: &str) -> Result<Option<Package>> {
        // Prefer local info when installed.
        for flag in ["-Qi", "-Si"] {
            let mut args = self.conf_args();
            args.push(flag.into());
            args.push(name.into());
            let out = run("pacman", &args).await?;
            if !out.success() || out.stdout.trim().is_empty() {
                continue;
            }
            let info = parse_package_info(&out.stdout);
            let mut pkg = Package {
                id: PackageId::new(PackageSource::Pacman, name),
                name: info.name.unwrap_or_else(|| name.to_string()),
                summary: info.description.clone().unwrap_or_else(|| name.to_string()),
                description: info.description.unwrap_or_default(),
                version: info.version.unwrap_or_default(),
                available_version: None,
                icon_name: Some("package-x-generic".into()),
                icon_url: None,
                publisher: info.packager.clone(),
                bug_url: None,
                donate_url: None,
                permissions: Some(tcms_core::i18n::t("perm.system_package")),
                is_proprietary: None,
                developer: info.packager,
                license: info.licenses,
                homepage: info.url,
                size_bytes: info.size_bytes,
                state: if flag == "-Qi" {
                    InstallState::Installed
                } else {
                    InstallState::Available
                },
                categories: Vec::new(),
            };
            pkg.apply_license_heuristics();
            return Ok(Some(pkg));
        }
        Ok(None)
    }
}

impl Default for PacmanBackend {
    fn default() -> Self {
        Self::new(true, String::new(), String::new())
    }
}

#[async_trait]
impl Backend for PacmanBackend {
    fn id(&self) -> BackendId {
        BackendId::Pacman
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn refresh(&self) -> Result<()> {
        self.ensure_enabled()?;
        let mut args = self.conf_args();
        args.push("-Sy".into());
        // Database sync needs privileges
        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        match run_privileged_pacman(&str_args, "pacman -Sy").await {
            Ok(_) => Ok(()),
            Err(err) => {
                tracing::warn!(error = %err, "pacman refresh without sync; continuing");
                Ok(())
            }
        }
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        self.ensure_enabled()?;
        if query.updates_only {
            let packages = self.updates().await?;
            return Ok(SearchResult {
                packages,
                truncated: false,
            });
        }
        if query.installed_only {
            let mut packages = self.installed().await?;
            if !query.text.is_empty() {
                let q = query.text.to_lowercase();
                packages.retain(|p| {
                    p.name.to_lowercase().contains(&q)
                        || p.summary.to_lowercase().contains(&q)
                        || p.id.id.to_lowercase().contains(&q)
                });
            }
            return Ok(SearchResult {
                packages,
                truncated: false,
            });
        }

        let packages = self.search_repos(&query.text).await?;
        Ok(SearchResult {
            packages,
            truncated: false,
        })
    }

    async fn get_package(&self, id: &PackageId) -> Result<Option<Package>> {
        if id.source != PackageSource::Pacman {
            return Ok(None);
        }
        if let Some(mut pkg) = self.query_info(&id.id).await? {
            pkg.apply_license_heuristics();
            return Ok(Some(pkg));
        }
        let results = self.search_repos(&id.id).await?;
        Ok(results.into_iter().find(|p| p.id.id == id.id))
    }

    async fn installed(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        let updates = self.query_updates().await.unwrap_or_default();
        let apps = discover_desktop_apps().await?;
        let mut seen = HashSet::new();
        let mut packages = Vec::new();
        for app in apps {
            if app.is_flatpak {
                continue;
            }
            let Some(pkg) = Self::package_from_desktop(&app, &updates) else {
                continue;
            };
            if seen.insert(pkg.id.id.clone()) {
                packages.push(pkg);
            }
        }
        packages.sort_by_key(|a| a.name.to_lowercase());
        Ok(packages)
    }

    async fn updates(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        let updates = self.query_updates().await?;
        let mut packages = Vec::new();
        for (name, new_ver) in updates {
            let mut args = self.conf_args();
            args.push("-Qi".into());
            args.push(name.clone());
            let info = run("pacman", &args)
                .await
                .unwrap_or(tcms_core::process::CommandOutput {
                    status: 1,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            let (desc, old_ver) = parse_qi(&info.stdout);
            packages.push(Package {
                id: PackageId::new(PackageSource::Pacman, &name),
                name: name.clone(),
                summary: desc.clone().unwrap_or_else(|| name.clone()),
                description: desc.unwrap_or_default(),
                version: old_ver.unwrap_or_else(|| "?".into()),
                available_version: Some(new_ver),
                icon_name: Some("package-x-generic".into()),
                icon_url: None,
                publisher: None,
                bug_url: None,
                donate_url: None,
                permissions: None,
                is_proprietary: None,
                developer: None,
                license: None,
                homepage: None,
                size_bytes: None,
                state: InstallState::Updatable,
                categories: Vec::new(),
            });
        }
        packages.sort_by_key(|a| a.name.to_lowercase());
        Ok(packages)
    }

    async fn install(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Pacman {
            return Err(Error::Message(format!(
                "pacman backend cannot install {}",
                id
            )));
        }
        tcms_core::assert_safe_package_id(id)?;
        let mut args = self.conf_args();
        args.push("-S".into());
        args.push("--noconfirm".into());
        args.push("--needed".into());
        args.extend(self.extra_arg_list());
        args.push(id.id.clone());
        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_privileged_pacman(&str_args, &format!("install {}", id.id)).await?;
        Ok(())
    }

    async fn remove(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Pacman {
            return Err(Error::Message(format!(
                "pacman backend cannot remove {}",
                id
            )));
        }
        tcms_core::assert_safe_package_id(id)?;
        let mut args = self.conf_args();
        args.push("-Rns".into());
        args.push("--noconfirm".into());
        args.extend(self.extra_arg_list());
        args.push(id.id.clone());
        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        run_privileged_pacman(&str_args, &format!("remove {}", id.id)).await?;
        Ok(())
    }
}

fn parse_qi(stdout: &str) -> (Option<String>, Option<String>) {
    let info = parse_package_info(stdout);
    (info.description, info.version)
}

#[derive(Default)]
struct PacmanInfo {
    name: Option<String>,
    description: Option<String>,
    version: Option<String>,
    url: Option<String>,
    licenses: Option<String>,
    packager: Option<String>,
    size_bytes: Option<u64>,
}

fn parse_size_bytes(value: &str) -> Option<u64> {
    // Examples: "12.34 MiB", "512.00 KiB", "1.20 GiB"
    let mut parts = value.split_whitespace();
    let num: f64 = parts.next()?.parse().ok()?;
    let unit = parts.next().unwrap_or("B").to_ascii_lowercase();
    let mult = if unit.starts_with("g") {
        1024.0 * 1024.0 * 1024.0
    } else if unit.starts_with("m") {
        1024.0 * 1024.0
    } else if unit.starts_with("k") {
        1024.0
    } else {
        1.0
    };
    Some((num * mult) as u64)
}

fn parse_package_info(stdout: &str) -> PacmanInfo {
    let mut info = PacmanInfo::default();
    for line in stdout.lines() {
        let Some(idx) = line.find(':') else {
            continue;
        };
        let key = line[..idx].trim();
        let value = line[idx + 1..].trim();
        if value.is_empty() || value == "None" {
            continue;
        }
        match key {
            "Name" => info.name = Some(value.to_string()),
            "Description" => info.description = Some(value.to_string()),
            "Version" => info.version = Some(value.to_string()),
            "URL" => info.url = Some(value.to_string()),
            "Licenses" => info.licenses = Some(value.to_string()),
            "Packager" => info.packager = Some(value.to_string()),
            "Installed Size" | "Download Size" => {
                info.size_bytes = info.size_bytes.or_else(|| parse_size_bytes(value));
            }
            _ => {}
        }
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_package_info_fields() {
        let raw = "\
Name            : firefox
Version         : 128.0-1
Description     : Standalone web browser
URL             : https://www.mozilla.org/firefox/
Licenses        : MPL-2.0
Packager        : Arch Linux <someone@archlinux.org>
Installed Size  : 12.50 MiB
";
        let info = parse_package_info(raw);
        assert_eq!(info.name.as_deref(), Some("firefox"));
        assert_eq!(info.version.as_deref(), Some("128.0-1"));
        assert_eq!(info.description.as_deref(), Some("Standalone web browser"));
        assert_eq!(
            info.url.as_deref(),
            Some("https://www.mozilla.org/firefox/")
        );
        assert_eq!(info.licenses.as_deref(), Some("MPL-2.0"));
        assert_eq!(info.size_bytes, Some(12 * 1024 * 1024 + 512 * 1024));
    }

    #[test]
    fn parse_size_units() {
        assert_eq!(parse_size_bytes("1.00 KiB"), Some(1024));
        assert_eq!(parse_size_bytes("2.00 MiB"), Some(2 * 1024 * 1024));
    }

    #[test]
    fn assert_safe_rejects_injection() {
        let id = PackageId::new(PackageSource::Pacman, "foo;rm -rf /");
        assert!(tcms_core::assert_safe_package_id(&id).is_err());
    }
}
