//! Flatpak / Flathub backend.

mod preview;
use std::collections::HashMap;

use async_trait::async_trait;
use tcms_core::process::run;
use tcms_core::{
    Backend, BackendId, Error, FlatpakInstallation, FlatpakRef, InstallState, Package, PackageId,
    PackageSource, Result, SearchQuery, SearchResult,
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
    native_arch: std::sync::Arc<tokio::sync::OnceCell<String>>,
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
            native_arch: Default::default(),
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

    pub fn installations(&self) -> [Self; 2] {
        ["user", "system"].map(|scope| {
            let mut b = self.clone();
            b.set_installation(scope);
            b
        })
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

    fn scope(&self) -> Result<FlatpakInstallation> {
        match self.installation.to_ascii_lowercase().as_str() {
            "user" => Ok(FlatpakInstallation::User),
            "system" => Ok(FlatpakInstallation::System),
            _ => Err(Error::Config(
                "Flatpak installation must be user or system".into(),
            )),
        }
    }

    async fn native_arch(&self) -> Result<&str> {
        self.native_arch
            .get_or_try_init(|| async {
                let out = run("flatpak", ["--default-arch"]).await?;
                out.ensure_success("flatpak --default-arch")?;
                let arch = out.stdout.trim().to_string();
                if !tcms_core::is_safe_pkg_token(&arch) {
                    return Err(Error::Message("invalid Flatpak architecture".into()));
                }
                Ok(arch)
            })
            .await
            .map(String::as_str)
    }

    /// Flathub home-page entries do not carry a CLI ref. Resolve them once,
    /// explicitly to Flathub stable in the selected installation.
    pub async fn catalog_id(&self, app_id: &str) -> Result<PackageId> {
        let mut id = PackageId::new(PackageSource::Flatpak, app_id);
        id.flatpak = Some(FlatpakRef {
            kind: "app".into(),
            arch: self.native_arch().await?.into(),
            branch: "stable".into(),
            origin: "flathub".into(),
            installation: self.scope()?,
        });
        id.flatpak_ref()?;
        Ok(id)
    }

    pub async fn download_updates(&self) -> Result<()> {
        self.ensure_enabled()?;
        let scope = self.scope()?;
        let args = [
            "update",
            "--no-deploy",
            "--noninteractive",
            "-y",
            scope.flag(),
        ];
        let out = if scope == FlatpakInstallation::User {
            run("flatpak", args).await?
        } else {
            tcms_core::process::run_privileged("flatpak", &args, "download Flatpak updates").await?
        };
        out.ensure_success("download Flatpak updates")
    }
    fn action_args(
        &self,
        action: &str,
        id: Option<&PackageId>,
    ) -> Result<(Vec<String>, FlatpakInstallation)> {
        let scope = match id {
            Some(id) => id
                .flatpak
                .as_ref()
                .ok_or_else(|| Error::Message("Flatpak reference has not been resolved".into()))?
                .installation
                .clone(),
            None => self.scope()?,
        };
        let mut args = vec![
            action.into(),
            "-y".into(),
            scope.flag().into(),
            "--noninteractive".into(),
        ];
        if let Some(id) = id {
            let reference = id.flatpak_ref()?;
            args.push("--".into());
            if action == "install" {
                args.push(id.flatpak.as_ref().unwrap().origin.clone());
            }
            args.push(reference);
        }
        Ok((args, scope))
    }

    async fn transact(&self, action: &str, id: Option<&PackageId>) -> Result<()> {
        self.ensure_enabled()?;
        let (args, scope) = self.action_args(action, id)?;
        let refs: Vec<_> = args.iter().map(String::as_str).collect();
        let out = match scope {
            FlatpakInstallation::User => run("flatpak", &refs).await?,
            FlatpakInstallation::System => {
                tcms_core::process::run_privileged("flatpak", &refs, "flatpak transaction").await?
            }
        };
        out.ensure_success(&format!("flatpak {action}"))
    }

    async fn list_refs(&self, kind: &str, updates: bool) -> Result<Vec<Package>> {
        let scope = self.scope()?;
        let filter = format!("--{kind}");
        let mut args = vec![
            if updates { "remote-ls" } else { "list" },
            scope.flag(),
            &filter,
            "--columns=application,arch,branch,origin,name,version,description",
        ];
        if updates {
            args.extend(["--updates", "--all"]);
        } else if kind == "runtime" {
            // Include locale/debug extensions, hidden by `list --runtime` alone.
            args.push("--all");
        }
        let out = run("flatpak", args).await?;
        out.ensure_success("flatpak list refs")?;
        parse_refs(&out.stdout, kind, scope)
    }

    async fn list_installed(&self) -> Result<Vec<Package>> {
        // Installed means local state; remote freshness belongs to Updates.
        let (mut apps, runtimes) = tokio::try_join!(
            self.list_refs("app", false),
            self.list_refs("runtime", false),
        )?;
        apps.extend(runtimes);
        Ok(apps)
    }

    async fn search_remote(&self, text: &str) -> Result<Vec<Package>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let scope = self.scope()?;
        // search has no --app/--arch option. It searches native AppStream data.
        let out = run(
            "flatpak",
            [
                "search",
                scope.flag(),
                "--columns=application,name,version,description,branch,remotes",
                "--",
                text,
            ],
        )
        .await?;
        out.ensure_success("flatpak search")?;
        if out.stdout.trim().is_empty() || out.stdout.trim() == "No matches found" {
            return Ok(vec![]);
        }
        parse_search(&out.stdout, self.native_arch().await?, scope)
    }

    async fn show_permissions(&self, id: &PackageId) -> Option<String> {
        let reference = id.flatpak_ref().ok()?;
        let scope = &id.flatpak.as_ref()?.installation;
        let out = run(
            "flatpak",
            ["info", "--show-permissions", scope.flag(), "--", &reference],
        )
        .await
        .ok()?;
        (out.success() && !out.stdout.trim().is_empty())
            .then(|| summarize_flatpak_permissions(&out.stdout))
    }

    pub async fn enrich_package(&self, mut pkg: Package) -> Package {
        if pkg
            .id
            .flatpak
            .as_ref()
            .is_some_and(|identity| identity.origin == "flathub")
        {
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
        }
        if pkg.permissions.is_none() {
            pkg.permissions = self.show_permissions(&pkg.id).await;
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
            out.ensure_success(&format!("flatpak remote-add {}", remote.name))?;
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
    fn label(&self) -> String {
        format!("flatpak({})", self.installation)
    }
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
        let out = run("flatpak", ["update", "--appstream", self.install_flag()]).await?;
        out.ensure_success("flatpak appstream refresh")
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
        self.ensure_enabled()?;
        if id.source != PackageSource::Flatpak {
            return Ok(None);
        }
        let resolved;
        let id = if id.flatpak.is_none() {
            resolved = self.catalog_id(&id.id).await?;
            &resolved
        } else {
            id
        };
        // Preserve the entry's installation even if Settings changed meanwhile.
        let mut backend = self.clone();
        backend.installation = id.flatpak.as_ref().unwrap().installation.label().into();
        if let Some(pkg) = backend
            .list_installed()
            .await?
            .into_iter()
            .find(|p| p.id == *id)
        {
            return Ok(Some(backend.enrich_package(pkg).await));
        }
        if let Some(pkg) = backend
            .search_remote(&id.id)
            .await?
            .into_iter()
            .find(|p| p.id == *id)
        {
            return Ok(Some(backend.enrich_package(pkg).await));
        }
        Ok(None)
    }

    async fn installed(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        self.list_installed().await
    }

    async fn updates(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        let mut packages = Vec::new();
        for kind in ["app", "runtime"] {
            let (local, remote) =
                tokio::try_join!(self.list_refs(kind, false), self.list_refs(kind, true))?;
            let available: HashMap<_, _> = remote.into_iter().map(|p| (p.id, p.version)).collect();
            for mut pkg in local {
                if let Some(version) = available.get(&pkg.id) {
                    pkg.state = InstallState::Updatable;
                    pkg.available_version = Some(version.clone());
                    packages.push(pkg);
                }
            }
        }
        Ok(packages)
    }

    async fn preview(
        &self,
        action: tcms_core::PackageAction,
        id: Option<&PackageId>,
    ) -> Result<tcms_core::TransactionPreview> {
        self.ensure_enabled()?;
        let scope = if let Some(id) = id {
            id.flatpak_ref()?;
            id.flatpak.as_ref().unwrap().installation.clone()
        } else {
            self.scope()?
        };
        let id = id.cloned();
        tokio::task::spawn_blocking(move || preview::resolve(scope, action, id))
            .await
            .map_err(|e| Error::Message(e.to_string()))?
    }
    async fn install(&self, id: &PackageId) -> Result<()> {
        self.transact("install", Some(id)).await
    }
    async fn update(&self, id: &PackageId) -> Result<()> {
        self.transact("update", Some(id)).await
    }
    async fn update_all(&self) -> Result<()> {
        self.transact("update", None).await
    }
    async fn remove(&self, id: &PackageId) -> Result<()> {
        self.transact("uninstall", Some(id)).await
    }
}

fn parse_refs(output: &str, kind: &str, installation: FlatpakInstallation) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let mut cols: Vec<_> = line.split('\t').collect();
        if cols.len() < 4 || cols.len() > 7 {
            return Err(Error::Message(format!(
                "unexpected Flatpak ref row: {line}"
            )));
        }
        cols.resize(7, "");
        let mut pkg = Package::stub(
            PackageSource::Flatpak,
            cols[0],
            if cols[4].is_empty() { cols[0] } else { cols[4] },
            cols[6],
            cols[5],
            InstallState::Installed,
        );
        pkg.id.flatpak = Some(FlatpakRef {
            kind: kind.into(),
            arch: cols[1].into(),
            branch: cols[2].into(),
            origin: cols[3].into(),
            installation: installation.clone(),
        });
        pkg.id.flatpak_ref()?;
        packages.push(pkg);
    }
    Ok(packages)
}

fn parse_search(
    output: &str,
    arch: &str,
    installation: FlatpakInstallation,
) -> Result<Vec<Package>> {
    let mut packages = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for line in output.lines().filter(|line| !line.trim().is_empty()) {
        let cols: Vec<_> = line.split('\t').collect();
        if cols.len() != 6 {
            return Err(Error::Message(format!(
                "unexpected Flatpak search row: {line}"
            )));
        }
        for remote in cols[5].split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let mut pkg = Package::stub(
                PackageSource::Flatpak,
                cols[0],
                cols[1],
                cols[3],
                cols[2],
                InstallState::Available,
            );
            pkg.id.flatpak = Some(FlatpakRef {
                kind: "app".into(),
                arch: arch.into(),
                branch: cols[4].into(),
                origin: remote.into(),
                installation: installation.clone(),
            });
            pkg.id.flatpak_ref()?;
            if seen.insert(pkg.id.clone()) {
                packages.push(pkg);
            }
        }
    }
    Ok(packages)
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

    fn identity(branch: &str, origin: &str, scope: FlatpakInstallation) -> PackageId {
        let mut id = PackageId::new(PackageSource::Flatpak, "org.example.App");
        id.flatpak = Some(FlatpakRef {
            kind: "app".into(),
            arch: "x86_64".into(),
            branch: branch.into(),
            origin: origin.into(),
            installation: scope,
        });
        id
    }

    #[test]
    fn transactions_keep_origin_branch_arch_and_original_scope() {
        let backend = FlatpakBackend::new(true, "system", "flathub|https://example.org");
        let id = identity("beta", "testing", FlatpakInstallation::User);
        let (args, _) = backend.action_args("install", Some(&id)).unwrap();
        assert_eq!(
            args,
            [
                "install",
                "-y",
                "--user",
                "--noninteractive",
                "--",
                "testing",
                "app/org.example.App/x86_64/beta"
            ]
        );
        for action in ["update", "uninstall"] {
            let (args, _) = backend.action_args(action, Some(&id)).unwrap();
            assert_eq!(args.last().unwrap(), "app/org.example.App/x86_64/beta");
            assert!(args.contains(&"--user".into()));
        }
        assert_eq!(
            backend.action_args("update", None).unwrap().0,
            ["update", "-y", "--system", "--noninteractive"]
        );
        assert!(backend
            .action_args(
                "install",
                Some(&PackageId::new(PackageSource::Flatpak, "org.example.App"))
            )
            .is_err());
    }

    #[test]
    fn search_preserves_multiple_remotes_and_branches() {
        let rows = "org.example.App\tExample\t1\tDescription\tstable\tflathub,testing\norg.example.App\tExample\t2\tDescription\tbeta\ttesting\n";
        let packages = parse_search(rows, "x86_64", FlatpakInstallation::User).unwrap();
        assert_eq!(packages.len(), 3);
        assert_ne!(packages[0].id, packages[1].id);
        assert_ne!(packages[1].id, packages[2].id);
        assert!(!tcms_core::packages_match(&packages[1], &packages[2]));
    }

    #[test]
    fn installed_refs_preserve_runtime_versions_and_empty_metadata() {
        let rows = "org.example.Platform\tx86_64\t24.08\tflathub\t\t\t\norg.example.Platform\tx86_64\t25.08\tflathub\tPlatform\t25\tRuntime\n";
        let packages = parse_refs(rows, "runtime", FlatpakInstallation::System).unwrap();
        assert_eq!(packages.len(), 2);
        assert_ne!(packages[0].id, packages[1].id);
        assert_eq!(
            packages[0].id.flatpak_ref().unwrap(),
            "runtime/org.example.Platform/x86_64/24.08"
        );
        assert!(parse_refs("malformed", "app", FlatpakInstallation::User).is_err());
    }

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
