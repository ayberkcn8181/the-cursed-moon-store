//! Flatpak / Flathub backend.

use std::collections::HashMap;

use async_trait::async_trait;
use tcms_core::process::run;
use tcms_core::{
    Backend, BackendId, Error, InstallState, Package, PackageId, PackageSource, Result,
    SearchQuery, SearchResult,
};

#[derive(Debug, Clone)]
pub struct FlatpakRemote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone)]
pub struct FlatpakBackend {
    enabled: bool,
    installation: String,
    remotes: Vec<FlatpakRemote>,
}

impl FlatpakBackend {
    pub fn new(enabled: bool, installation: impl Into<String>, remotes_text: &str) -> Self {
        let remotes = remotes_text
            .lines()
            .filter_map(|line| {
                let line = line.trim();
                if line.is_empty() {
                    return None;
                }
                let (name, url) = line.split_once('|')?;
                Some(FlatpakRemote {
                    name: name.trim().to_string(),
                    url: url.trim().to_string(),
                })
            })
            .collect();
        Self {
            enabled,
            installation: installation.into(),
            remotes,
        }
    }

    pub fn remotes(&self) -> &[FlatpakRemote] {
        &self.remotes
    }

    pub fn set_remotes_from_text(&mut self, remotes_text: &str) {
        *self = Self::new(self.enabled, self.installation.clone(), remotes_text);
    }

    pub fn installation(&self) -> &str {
        &self.installation
    }

    pub fn set_installation(&mut self, installation: impl Into<String>) {
        self.installation = installation.into();
    }

    fn ensure_enabled(&self) -> Result<()> {
        if self.enabled {
            Ok(())
        } else {
            Err(Error::BackendDisabled("flatpak".into()))
        }
    }

    fn install_flag(&self) -> &str {
        if self.installation.eq_ignore_ascii_case("user") {
            "--user"
        } else {
            "--system"
        }
    }

    fn default_remote(&self) -> &str {
        self.remotes
            .first()
            .map(|r| r.name.as_str())
            .unwrap_or("flathub")
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        let out = run(
            "flatpak",
            [
                "list",
                "--app",
                self.install_flag(),
                "--columns=application,name,version,description",
            ],
        )
        .await?;
        if !out.success() {
            // empty install is fine
            if out.stdout.trim().is_empty() {
                return Ok(Vec::new());
            }
            out.ensure_success("flatpak list")?;
        }

        let updates = self.update_map().await.unwrap_or_default();
        let mut packages = Vec::new();
        for line in out.stdout.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.is_empty() || cols[0].trim().is_empty() {
                continue;
            }
            let app_id = cols[0].trim().to_string();
            let name = cols
                .get(1)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(&app_id);
            let version = cols.get(2).map(|s| s.trim()).unwrap_or("").to_string();
            let summary = cols.get(3).map(|s| s.trim()).unwrap_or("").to_string();
            let state = if updates.contains_key(&app_id) {
                InstallState::Updatable
            } else {
                InstallState::Installed
            };
            packages.push(Package {
                id: PackageId::new(PackageSource::Flatpak, &app_id),
                name: name.to_string(),
                summary: if summary.is_empty() {
                    app_id.clone()
                } else {
                    summary.clone()
                },
                description: summary,
                version,
                available_version: updates.get(&app_id).cloned(),
                icon_name: Some("application-x-executable".into()),
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
                installed_elsewhere: false,
                categories: Vec::new(),
            });
        }
        packages.sort_by_key(|a| a.name.to_lowercase());
        Ok(packages)
    }

    async fn update_map(&self) -> Result<HashMap<String, String>> {
        let out = run(
            "flatpak",
            [
                "remote-ls",
                "--updates",
                "--app",
                self.install_flag(),
                "--columns=application,version",
            ],
        )
        .await?;
        let mut map = HashMap::new();
        if !out.success() {
            return Ok(map);
        }
        for line in out.stdout.lines() {
            let cols: Vec<&str> = line.split('\t').collect();
            if cols.len() >= 2 {
                map.insert(cols[0].trim().to_string(), cols[1].trim().to_string());
            } else if cols.len() == 1 && !cols[0].trim().is_empty() {
                map.insert(cols[0].trim().to_string(), String::new());
            }
        }
        Ok(map)
    }

    async fn search_remote(&self, text: &str) -> Result<Vec<Package>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        // Prefer apps-only search when the flatpak CLI supports --app.
        let out = match run(
            "flatpak",
            [
                "search",
                "--app",
                "--columns=application,name,version,description,branch",
                text,
            ],
        )
        .await
        {
            Ok(o) if o.success() || !o.stdout.trim().is_empty() => o,
            _ => {
                run(
                    "flatpak",
                    [
                        "search",
                        "--columns=application,name,version,description,branch",
                        text,
                    ],
                )
                .await?
            }
        };
        if !out.success() && out.stdout.trim().is_empty() {
            return Ok(Vec::new());
        }

        let mut packages = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for line in out.stdout.lines() {
            if line.starts_with("Application ID") || line.starts_with("ID") {
                continue;
            }
            let cols: Vec<&str> = if line.contains('\t') {
                line.split('\t').collect()
            } else {
                line.split("  ")
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .collect()
            };
            if cols.is_empty() {
                continue;
            }
            let app_id = cols[0].trim().to_string();
            if app_id.is_empty() || !seen.insert(app_id.clone()) {
                continue;
            }
            let name = cols
                .get(1)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or(&app_id)
                .to_string();
            let version = cols
                .get(2)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            let summary = cols
                .get(3)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();

            packages.push(Package {
                id: PackageId::new(PackageSource::Flatpak, &app_id),
                name,
                summary: summary.clone(),
                description: summary,
                version,
                available_version: None,
                icon_name: Some("application-x-executable".into()),
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
                state: InstallState::Available,
                installed_elsewhere: false,
                categories: Vec::new(),
            });

            if packages.len() >= 60 {
                break;
            }
        }
        Ok(packages)
    }

    async fn show_permissions(&self, app_id: &str) -> Option<String> {
        let out = run(
            "flatpak",
            ["info", "--show-permissions", self.install_flag(), app_id],
        )
        .await
        .ok();
        if let Some(out) = out {
            if out.success() && !out.stdout.trim().is_empty() {
                return Some(summarize_flatpak_permissions(&out.stdout));
            }
        }
        let out = run("flatpak", ["info", "--show-permissions", app_id])
            .await
            .ok()?;
        if out.success() && !out.stdout.trim().is_empty() {
            return Some(summarize_flatpak_permissions(&out.stdout));
        }
        Some(tcms_core::i18n::t("detail.permissions_sandbox"))
    }

    pub async fn enrich_package(&self, mut pkg: Package) -> Package {
        if let Ok(meta) = tcms_core::fetch_flathub_app(&pkg.id.id).await {
            if pkg.summary.is_empty() {
                pkg.summary = meta.summary;
            }
            if pkg.description.is_empty() || pkg.description == pkg.name {
                pkg.description = meta.description;
            }
            if pkg.icon_url.is_none() {
                pkg.icon_url = meta.icon_url;
            }
            if pkg.developer.is_none() {
                pkg.developer = meta.developer.clone();
            }
            if pkg.publisher.is_none() {
                pkg.publisher = meta.publisher.or(meta.developer);
            }
            if pkg.license.is_none() {
                pkg.license = meta.license;
            }
            if pkg.homepage.is_none() {
                pkg.homepage = meta.homepage;
            }
            if pkg.bug_url.is_none() {
                pkg.bug_url = meta.bug_url;
            }
            if pkg.donate_url.is_none() {
                pkg.donate_url = meta.donate_url;
            }
            if pkg.is_proprietary.is_none() {
                pkg.is_proprietary = meta.is_proprietary;
            }
            if pkg.size_bytes.is_none() {
                pkg.size_bytes = meta.size_bytes;
            }
        }
        if pkg.permissions.is_none() {
            pkg.permissions = self.show_permissions(&pkg.id.id).await;
        }
        pkg.apply_license_heuristics();
        pkg
    }

    async fn ensure_configured_remotes(&self) -> Result<()> {
        for remote in &self.remotes {
            if remote.name.is_empty() || remote.url.is_empty() {
                continue;
            }
            let listed = run(
                "flatpak",
                ["remotes", self.install_flag(), "--columns=name"],
            )
            .await;
            let exists = listed
                .as_ref()
                .map(|o| {
                    o.stdout
                        .lines()
                        .any(|l| l.trim().eq_ignore_ascii_case(&remote.name))
                })
                .unwrap_or(false);
            if exists {
                continue;
            }
            let args = [
                "remote-add",
                "--if-not-exists",
                self.install_flag(),
                remote.name.as_str(),
                remote.url.as_str(),
            ];
            let out = if self.installation.eq_ignore_ascii_case("user") {
                run("flatpak", args).await?
            } else {
                tcms_core::process::run_privileged(
                    "flatpak",
                    &args,
                    &format!("flatpak remote-add {}", remote.name),
                )
                .await?
            };
            if !out.success() {
                tracing::warn!(
                    remote = %remote.name,
                    stderr = %out.stderr,
                    "failed to add flatpak remote"
                );
            }
        }
        Ok(())
    }
}

impl Default for FlatpakBackend {
    fn default() -> Self {
        Self::new(
            true,
            "system",
            "flathub|https://dl.flathub.org/repo/flathub.flatpakrepo",
        )
    }
}

#[async_trait]
impl Backend for FlatpakBackend {
    fn id(&self) -> BackendId {
        BackendId::Flatpak
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn refresh(&self) -> Result<()> {
        self.ensure_enabled()?;
        self.ensure_configured_remotes().await?;
        let _ = run("flatpak", ["update", "--appstream", self.install_flag()]).await;
        Ok(())
    }

    async fn search(&self, query: &SearchQuery) -> Result<SearchResult> {
        self.ensure_enabled()?;
        if query.updates_only {
            return Ok(SearchResult {
                packages: self.updates().await?,
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
        Ok(SearchResult {
            packages: self.search_remote(&query.text).await?,
            truncated: false,
        })
    }

    async fn get_package(&self, id: &PackageId) -> Result<Option<Package>> {
        if id.source != PackageSource::Flatpak {
            return Ok(None);
        }
        if let Some(pkg) = self
            .list_installed()
            .await?
            .into_iter()
            .find(|p| p.id.id == id.id)
        {
            return Ok(Some(self.enrich_package(pkg).await));
        }
        let found = self.search_remote(&id.id).await?;
        if let Some(pkg) = found.into_iter().find(|p| p.id.id == id.id) {
            return Ok(Some(self.enrich_package(pkg).await));
        }
        // Fall back to Flathub API even if local search misses.
        match tcms_core::fetch_flathub_app(&id.id).await {
            Ok(pkg) => Ok(Some(self.enrich_package(pkg).await)),
            Err(_) => Ok(None),
        }
    }

    async fn installed(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        self.list_installed().await
    }

    async fn updates(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        let updates = self.update_map().await?;
        let installed = self.list_installed().await?;
        Ok(installed
            .into_iter()
            .filter_map(|mut p| {
                if let Some(ver) = updates.get(&p.id.id) {
                    p.state = InstallState::Updatable;
                    p.available_version = Some(ver.clone());
                    Some(p)
                } else {
                    None
                }
            })
            .collect())
    }

    async fn install(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Flatpak {
            return Err(Error::Message(format!(
                "flatpak backend cannot install {}",
                id
            )));
        }
        tcms_core::assert_safe_package_id(id)?;
        let remote = self.default_remote();
        let args = ["install", "-y", self.install_flag(), remote, id.id.as_str()];
        let out = if self.installation.eq_ignore_ascii_case("user") {
            run("flatpak", args).await?
        } else {
            tcms_core::process::run_privileged(
                "flatpak",
                &args,
                &format!("flatpak install {}", id.id),
            )
            .await?
        };
        out.ensure_success(&format!("flatpak install {}", id.id))?;
        Ok(())
    }

    async fn remove(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Flatpak {
            return Err(Error::Message(format!(
                "flatpak backend cannot remove {}",
                id
            )));
        }
        tcms_core::assert_safe_package_id(id)?;
        let args = ["uninstall", "-y", self.install_flag(), id.id.as_str()];
        let out = if self.installation.eq_ignore_ascii_case("user") {
            run("flatpak", args).await?
        } else {
            tcms_core::process::run_privileged(
                "flatpak",
                &args,
                &format!("flatpak uninstall {}", id.id),
            )
            .await?
        };
        out.ensure_success(&format!("flatpak uninstall {}", id.id))?;
        Ok(())
    }
}

fn summarize_flatpak_permissions(raw: &str) -> String {
    use tcms_core::i18n::t;
    let mut bits = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("shared=") {
            bits.push(format!("{}: {}", t("perm.shared"), rest.replace(';', ", ")));
        } else if let Some(rest) = line.strip_prefix("sockets=") {
            bits.push(format!(
                "{}: {}",
                t("perm.sockets"),
                rest.replace(';', ", ")
            ));
        } else if let Some(rest) = line.strip_prefix("devices=") {
            bits.push(format!(
                "{}: {}",
                t("perm.devices"),
                rest.replace(';', ", ")
            ));
        } else if let Some(rest) = line.strip_prefix("filesystems=") {
            bits.push(format!(
                "{}: {}",
                t("perm.filesystems"),
                rest.replace(';', ", ")
            ));
        } else if line.contains("=talk") || line.contains("=own") {
            bits.push(line.to_string());
        }
    }
    if bits.is_empty() {
        raw.chars().take(400).collect()
    } else {
        bits.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_permissions_extracts_sections() {
        let raw = "\
[Context]
shared=network;ipc
sockets=x11;wayland
devices=dri
filesystems=xdg-download;home
";
        let summary = summarize_flatpak_permissions(raw);
        assert!(summary.contains("network"));
        assert!(summary.contains("wayland") || summary.contains("x11"));
        assert!(summary.contains("xdg-download") || summary.contains("home"));
    }

    #[test]
    fn remote_parsing() {
        let backend = FlatpakBackend::new(
            true,
            "user",
            "flathub|https://dl.flathub.org/repo/flathub.flatpakrepo\n\nbadline\n",
        );
        assert_eq!(backend.remotes().len(), 1);
        assert_eq!(backend.remotes()[0].name, "flathub");
        assert_eq!(backend.install_flag(), "--user");
    }
}
