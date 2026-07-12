//! AUR backend via RPC search and paru/yay/pacman for transactions.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde::Deserialize;
use tcms_core::process::run;
use tcms_core::{
    urlencoding, Backend, BackendId, Error, InstallState, Package, PackageId, PackageSource,
    Result, SearchQuery, SearchResult,
};

#[derive(Debug, Clone)]
pub struct AurBackend {
    enabled: bool,
    rpc_url: String,
    helper: String,
    extra_args: String,
}

#[derive(Debug, Deserialize)]
struct AurSearchResponse {
    results: Vec<AurPkg>,
}

#[derive(Debug, Deserialize)]
struct AurInfoResponse {
    results: Vec<AurPkg>,
}

#[derive(Debug, Deserialize)]
struct AurPkg {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Description")]
    description: Option<String>,
    #[serde(rename = "Version")]
    version: String,
    #[serde(rename = "Maintainer")]
    maintainer: Option<String>,
    #[serde(rename = "URL")]
    url: Option<String>,
    #[serde(rename = "License")]
    license: Option<Vec<String>>,
    #[serde(rename = "NumVotes")]
    num_votes: Option<i64>,
    #[serde(rename = "Popularity")]
    popularity: Option<f64>,
}

impl AurBackend {
    pub fn new(
        enabled: bool,
        rpc_url: impl Into<String>,
        helper: impl Into<String>,
        extra_args: impl Into<String>,
    ) -> Self {
        Self {
            enabled,
            rpc_url: rpc_url.into(),
            helper: helper.into(),
            extra_args: extra_args.into(),
        }
    }

    pub fn rpc_url(&self) -> &str {
        &self.rpc_url
    }

    pub fn set_rpc_url(&mut self, url: impl Into<String>) {
        self.rpc_url = url.into();
    }

    pub fn helper(&self) -> &str {
        &self.helper
    }

    pub fn set_helper(&mut self, helper: impl Into<String>) {
        self.helper = helper.into();
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
            Err(Error::BackendDisabled("aur".into()))
        }
    }

    fn extra_arg_list(&self) -> Vec<String> {
        self.extra_args
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    async fn resolve_helper(&self) -> Result<String> {
        for candidate in [self.helper.as_str(), "paru", "yay"] {
            if candidate.is_empty() || candidate == "makepkg" {
                continue;
            }
            let path = tcms_core::process::resolve_program(candidate);
            if path.is_file() {
                return Ok(path.to_string_lossy().into_owned());
            }
        }
        Err(Error::Message(
            "No AUR helper found (install paru or yay). makepkg alone is not supported from the store UI."
                .into(),
        ))
    }

    async fn foreign_packages(&self) -> Result<HashMap<String, String>> {
        let out = run("pacman", ["-Qm"]).await?;
        let mut map = HashMap::new();
        if !out.success() && out.status != 1 {
            return Ok(map);
        }
        for line in out.stdout.lines() {
            let mut parts = line.split_whitespace();
            if let (Some(name), Some(ver)) = (parts.next(), parts.next()) {
                map.insert(name.to_string(), ver.to_string());
            }
        }
        Ok(map)
    }

    async fn rpc_search(&self, text: &str) -> Result<Vec<AurPkg>> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let base = self.rpc_url.trim_end_matches('/');
        // Support both .../rpc and .../rpc/v5
        let url = if base.ends_with("/v5") {
            format!("{base}/search/{}?by=name-desc", urlencoding(text))
        } else {
            format!("{base}/v5/search/{}?by=name-desc", urlencoding(text))
        };
        let client = reqwest::Client::builder()
            .user_agent("TheCursedMoonStore/0.1")
            .build()
            .map_err(|e| Error::Message(e.to_string()))?;
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Message(format!("AUR RPC request failed: {e}")))?;
        if !resp.status().is_success() {
            return Err(Error::Message(format!("AUR RPC HTTP {}", resp.status())));
        }
        let body: AurSearchResponse = resp
            .json()
            .await
            .map_err(|e| Error::Message(format!("AUR RPC parse failed: {e}")))?;
        Ok(body.results)
    }

    async fn rpc_info(&self, names: &[String]) -> Result<HashMap<String, AurPkg>> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }
        let base = self.rpc_url.trim_end_matches('/');
        let endpoint = if base.ends_with("/v5") {
            format!("{base}/info")
        } else {
            format!("{base}/v5/info")
        };
        let client = reqwest::Client::builder()
            .user_agent("TheCursedMoonStore/0.1")
            .build()
            .map_err(|e| Error::Message(e.to_string()))?;

        let mut map = HashMap::new();
        // Batch in chunks of 100
        for chunk in names.chunks(100) {
            let mut req = client.get(&endpoint);
            for name in chunk {
                req = req.query(&[("arg[]", name.as_str())]);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| Error::Message(format!("AUR info failed: {e}")))?;
            if !resp.status().is_success() {
                continue;
            }
            let body: AurInfoResponse = resp
                .json()
                .await
                .map_err(|e| Error::Message(format!("AUR info parse failed: {e}")))?;
            for pkg in body.results {
                map.insert(pkg.name.clone(), pkg);
            }
        }
        Ok(map)
    }

    async fn vercmp_newer(remote: &str, local: &str) -> bool {
        let out = run("vercmp", [remote, local]).await.ok();
        out.map(|o| o.stdout.trim() == "1").unwrap_or(false)
    }

    fn to_package(pkg: &AurPkg, state: InstallState, local_version: Option<String>) -> Package {
        let summary = pkg.description.clone().unwrap_or_else(|| pkg.name.clone());
        let mut pkg = Package {
            id: PackageId::new(PackageSource::Aur, &pkg.name),
            name: pkg.name.clone(),
            summary: summary.clone(),
            description: summary,
            version: local_version.unwrap_or_else(|| pkg.version.clone()),
            available_version: if state == InstallState::Updatable {
                Some(pkg.version.clone())
            } else {
                None
            },
            icon_name: Some("package-x-generic".into()),
            icon_url: None,
            publisher: pkg.maintainer.clone(),
            bug_url: Some(format!("https://aur.archlinux.org/packages/{}", pkg.name)),
            donate_url: None,
            permissions: Some(tcms_core::i18n::t("perm.aur_package")),
            is_proprietary: None,
            developer: pkg.maintainer.clone(),
            license: pkg.license.as_ref().map(|l| l.join(", ")),
            homepage: pkg.url.clone(),
            size_bytes: None,
            state,
            categories: vec!["AUR".into()],
        };
        pkg.apply_license_heuristics();
        pkg
    }
}

impl Default for AurBackend {
    fn default() -> Self {
        Self::new(true, "https://aur.archlinux.org/rpc", "paru", String::new())
    }
}

#[async_trait]
impl Backend for AurBackend {
    fn id(&self) -> BackendId {
        BackendId::Aur
    }

    fn enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    async fn refresh(&self) -> Result<()> {
        self.ensure_enabled()?;
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

        let foreign = self.foreign_packages().await.unwrap_or_default();
        let mut results = self.rpc_search(&query.text).await?;
        // Prefer popular hits so the first page feels useful.
        results.sort_by(|a, b| {
            b.num_votes
                .unwrap_or(0)
                .cmp(&a.num_votes.unwrap_or(0))
                .then_with(|| {
                    b.popularity
                        .unwrap_or(0.0)
                        .partial_cmp(&a.popularity.unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
        });
        let mut packages = Vec::new();
        for pkg in results.into_iter().take(40) {
            // Skip expensive vercmp during search; Updates tab still does full checks.
            let state = if foreign.contains_key(&pkg.name) {
                InstallState::Installed
            } else {
                InstallState::Available
            };
            let local = foreign.get(&pkg.name).cloned();
            packages.push(Self::to_package(&pkg, state, local));
        }
        Ok(SearchResult {
            truncated: packages.len() >= 40,
            packages,
        })
    }

    async fn get_package(&self, id: &PackageId) -> Result<Option<Package>> {
        if id.source != PackageSource::Aur {
            return Ok(None);
        }
        let info = self.rpc_info(std::slice::from_ref(&id.id)).await?;
        let Some(pkg) = info.get(&id.id) else {
            return Ok(None);
        };
        let foreign = self.foreign_packages().await.unwrap_or_default();
        let state = if let Some(local) = foreign.get(&id.id) {
            if Self::vercmp_newer(&pkg.version, local).await {
                InstallState::Updatable
            } else {
                InstallState::Installed
            }
        } else {
            InstallState::Available
        };
        Ok(Some(Self::to_package(
            pkg,
            state,
            foreign.get(&id.id).cloned(),
        )))
    }

    async fn installed(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        let foreign = self.foreign_packages().await?;
        let names: Vec<String> = foreign.keys().cloned().collect();
        let info = self.rpc_info(&names).await.unwrap_or_default();
        let mut packages = Vec::new();
        let mut seen = HashSet::new();
        for (name, local_ver) in &foreign {
            if !seen.insert(name.clone()) {
                continue;
            }
            if let Some(pkg) = info.get(name) {
                let state = if Self::vercmp_newer(&pkg.version, local_ver).await {
                    InstallState::Updatable
                } else {
                    InstallState::Installed
                };
                packages.push(Self::to_package(pkg, state, Some(local_ver.clone())));
            } else {
                packages.push(Package {
                    id: PackageId::new(PackageSource::Aur, name),
                    name: name.clone(),
                    summary: tcms_core::i18n::t_args("aur.foreign_summary", &[("name", name)]),
                    description: String::new(),
                    version: local_ver.clone(),
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
                    state: InstallState::Installed,
                    categories: vec!["AUR".into()],
                });
            }
        }
        packages.sort_by_key(|a| a.name.to_lowercase());
        Ok(packages)
    }

    async fn updates(&self) -> Result<Vec<Package>> {
        self.ensure_enabled()?;
        Ok(self
            .installed()
            .await?
            .into_iter()
            .filter(|p| p.state == InstallState::Updatable)
            .collect())
    }

    async fn install(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Aur {
            return Err(Error::Message(format!("AUR backend cannot install {}", id)));
        }
        tcms_core::assert_safe_package_id(id)?;
        let helper = self.resolve_helper().await?;
        let helper_name = std::path::Path::new(&helper)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(helper.as_str());

        // GUI installs have no TTY: skip interactive PKGBUILD review and route
        // privilege escalation through pkexec so Polkit can prompt.
        let mut args = vec![
            "-S".to_string(),
            "--noconfirm".to_string(),
            "--needed".to_string(),
        ];
        if helper_name == "paru" || helper_name.ends_with("paru") {
            args.push("--skipreview".into());
        }
        if let Some(pkexec) = tcms_core::process::pkexec_path() {
            args.push("--sudo".into());
            args.push(pkexec.to_string_lossy().into_owned());
        }
        args.extend(self.extra_arg_list());
        args.push(id.id.clone());
        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        let out = run(&helper, &str_args).await?;
        out.ensure_success(&format!("{helper_name} install {}", id.id))?;
        Ok(())
    }

    async fn remove(&self, id: &PackageId) -> Result<()> {
        self.ensure_enabled()?;
        if id.source != PackageSource::Aur {
            return Err(Error::Message(format!("AUR backend cannot remove {}", id)));
        }
        tcms_core::assert_safe_package_id(id)?;
        // Removal is still pacman; do not append AUR-helper-specific extra args.
        let args = ["-Rns".to_string(), "--noconfirm".to_string(), id.id.clone()];
        let str_args: Vec<&str> = args.iter().map(String::as_str).collect();
        tcms_core::process::run_privileged_pacman(&str_args, &format!("remove {}", id.id)).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tcms_core::urlencoding;

    #[test]
    fn urlencoding_basic() {
        assert_eq!(urlencoding("firefox"), "firefox");
        assert_eq!(urlencoding("foo bar"), "foo%20bar");
        assert_eq!(urlencoding("a+b"), "a%2Bb");
    }

    #[test]
    fn parse_aur_search_json() {
        let raw = r#"{
          "resultcount": 1,
          "results": [{
            "Name": "spotify",
            "Description": "Music",
            "Version": "1.2.3-1",
            "Maintainer": "someone",
            "URL": "https://spotify.com",
            "License": ["proprietary"],
            "NumVotes": 10,
            "Popularity": 1.5
          }]
        }"#;
        let parsed: AurSearchResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(parsed.results.len(), 1);
        assert_eq!(parsed.results[0].name, "spotify");
        assert_eq!(parsed.results[0].version, "1.2.3-1");
    }
}
