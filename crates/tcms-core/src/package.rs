use serde::{Deserialize, Serialize};
use std::fmt;

/// Which software source a package comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageSource {
    Pacman,
    Flatpak,
    Aur,
}

impl PackageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pacman => "pacman",
            Self::Flatpak => "flatpak",
            Self::Aur => "aur",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Pacman => "System",
            Self::Flatpak => "Flathub",
            Self::Aur => "AUR",
        }
    }

    pub fn i18n_key(self) -> &'static str {
        match self {
            Self::Pacman => "source.pacman",
            Self::Flatpak => "source.flatpak",
            Self::Aur => "source.aur",
        }
    }

    pub fn from_str_loose(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pacman" | "arch" | "system" | "repo" | "sistem" => Some(Self::Pacman),
            "flatpak" | "flathub" => Some(Self::Flatpak),
            "aur" => Some(Self::Aur),
            _ => None,
        }
    }
}

impl fmt::Display for PackageSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    pub source: PackageSource,
    pub id: String,
}

impl PackageId {
    pub fn new(source: PackageSource, id: impl Into<String>) -> Self {
        Self {
            source,
            id: id.into(),
        }
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source.as_str(), self.id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    Available,
    Installed,
    Updatable,
    Installing,
    Removing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    pub id: PackageId,
    pub name: String,
    pub summary: String,
    pub description: String,
    pub version: String,
    pub available_version: Option<String>,
    /// Theme icon name or local file path under the icon cache.
    pub icon_name: Option<String>,
    /// Remote icon URL (downloaded lazily into the cache).
    pub icon_url: Option<String>,
    pub developer: Option<String>,
    pub publisher: Option<String>,
    pub license: Option<String>,
    pub homepage: Option<String>,
    pub bug_url: Option<String>,
    /// Donation / tip URL when available (e.g. Flathub).
    pub donate_url: Option<String>,
    /// Human-readable permission summary (mainly Flatpak).
    pub permissions: Option<String>,
    /// `Some(true)` = proprietary, `Some(false)` = open source, `None` = unknown.
    pub is_proprietary: Option<bool>,
    pub size_bytes: Option<u64>,
    pub state: InstallState,
    pub categories: Vec<String>,
}

impl Package {
    pub fn stub(
        source: PackageSource,
        id: &str,
        name: &str,
        summary: &str,
        version: &str,
        state: InstallState,
    ) -> Self {
        Self {
            id: PackageId::new(source, id),
            name: name.to_string(),
            summary: summary.to_string(),
            description: summary.to_string(),
            version: version.to_string(),
            available_version: None,
            icon_name: Some("application-x-executable".into()),
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
            state,
            categories: Vec::new(),
        }
    }

    pub fn display_publisher(&self) -> Option<&str> {
        self.publisher
            .as_deref()
            .or(self.developer.as_deref())
            .filter(|s| !s.is_empty())
    }

    pub fn apply_license_heuristics(&mut self) {
        if self.is_proprietary.is_none() {
            self.is_proprietary = license_is_proprietary(self.license.as_deref());
        }
        if self.bug_url.is_none() {
            if let Some(home) = self.homepage.as_deref() {
                let base = home.trim_end_matches('/');
                if base.contains("github.com/") {
                    self.bug_url = Some(format!("{base}/issues"));
                } else if base.contains("gitlab.com/") || base.contains("gitlab.") {
                    self.bug_url = Some(format!("{base}/-/issues"));
                }
            }
        }
        if let Some(desc) = strip_simple_html(&self.description) {
            self.description = desc;
        }
    }
}

/// Strip a few common HTML tags from Flathub descriptions.
pub fn strip_simple_html(input: &str) -> Option<String> {
    if !input.contains('<') {
        return None;
    }
    let mut out = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    let cleaned = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ");
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    Some(cleaned)
}

/// Infer proprietary vs open-source from a license string.
pub fn license_is_proprietary(license: Option<&str>) -> Option<bool> {
    let lic = license.map(str::trim).filter(|s| !s.is_empty())?;
    let lower = lic.to_ascii_lowercase();
    if lower.contains("proprietary")
        || lower.contains("licenseref-proprietary")
        || lower == "custom"
        || lower.contains("commercial")
    {
        return Some(true);
    }
    const OSS: &[&str] = &[
        "gpl",
        "lgpl",
        "agpl",
        "mit",
        "bsd",
        "apache",
        "mpl",
        "isc",
        "zlib",
        "artistic",
        "cc0",
        "unlicense",
        "wtfpl",
        "epl",
        "cddl",
        "openssl",
        "python",
        "php",
        "ruby",
    ];
    if OSS.iter().any(|t| lower.contains(t)) {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn license_detection() {
        assert_eq!(license_is_proprietary(Some("MIT")), Some(false));
        assert_eq!(
            license_is_proprietary(Some("GPL-3.0-or-later")),
            Some(false)
        );
        assert_eq!(
            license_is_proprietary(Some("LicenseRef-proprietary")),
            Some(true)
        );
        assert_eq!(license_is_proprietary(None), None);
    }

    #[test]
    fn html_strip() {
        let out = strip_simple_html("<p>Hello <b>world</b>&nbsp;!</p>").unwrap();
        assert_eq!(out, "Hello world !");
    }

    #[test]
    fn gitlab_bug_url() {
        let mut p = Package::stub(
            PackageSource::Pacman,
            "foo",
            "Foo",
            "",
            "1",
            InstallState::Available,
        );
        p.homepage = Some("https://gitlab.com/group/project".into());
        p.apply_license_heuristics();
        assert_eq!(
            p.bug_url.as_deref(),
            Some("https://gitlab.com/group/project/-/issues")
        );
    }
}
