//! Cross-source app identity helpers for install priority and search.

use crate::package::{Package, PackageId};

/// Build a search string that works across pacman / Flatpak / AUR.
pub fn search_text_for(pkg: &Package) -> String {
    let name = pkg.name.trim();
    if !name.is_empty() && name.len() < 80 && !name.contains('.') {
        return name.to_string();
    }
    if pkg.id.id.contains('.') {
        if let Some(last) = pkg.id.id.rsplit('.').next() {
            if !last.is_empty() {
                return last.to_string();
            }
        }
    }
    pkg.id.id.clone()
}

/// Normalize an app name/id for equality checks.
pub fn normalize_app_key(name: &str, id: &str) -> String {
    let mut s = name.trim().to_ascii_lowercase();
    if s.is_empty() {
        s = id.trim().to_ascii_lowercase();
    }
    if s.contains('.') {
        if let Some(last) = s.rsplit('.').next() {
            s = last.to_string();
        }
    }
    // Drop common packaging suffixes for comparison.
    for suffix in [
        "-bin",
        "-git",
        "-svn",
        "-hg",
        "-bzr",
        "-appimage",
        "-flatpak",
    ] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            if stripped.len() >= 3 {
                s = stripped.to_string();
                break;
            }
        }
    }
    s.chars().filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Whether two packages likely refer to the same end-user application.
///
/// Prefer exact normalized equality. Only allow a short, explicit alias list —
/// never fuzzy prefix matching (that wrongly maps `rust` → `rustdesk`).
pub fn packages_match(original: &Package, candidate: &Package) -> bool {
    if candidate.id == original.id {
        return true;
    }
    let a = normalize_app_key(&original.name, &original.id.id);
    let b = normalize_app_key(&candidate.name, &candidate.id.id);
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    known_aliases(&a, &b)
}

fn known_aliases(a: &str, b: &str) -> bool {
    const ALIASES: &[&[&str]] = &[
        &["firefox", "firefoxesr", "firefoxbin"],
        &["chromium", "googlechrome", "chrome"],
        &["code", "vscodium", "visualstudiocode", "codium"],
        &["libreoffice", "libreofficefresh", "libreofficestill"],
        &["telegram", "telegramdesktop"],
        &["discord"],
        &["spotify"],
        &["gimp"],
        &["inkscape"],
        &["vlc"],
        &["obs", "obsstudio"],
        &["steam", "steamnative"],
    ];
    ALIASES
        .iter()
        .any(|group| group.contains(&a) && group.contains(&b))
}

/// Validate a package id/name before privileged operations.
pub fn is_safe_pkg_token(s: &str) -> bool {
    !s.is_empty()
        && s.len() < 256
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-'))
}

pub fn assert_safe_package_id(id: &PackageId) -> crate::Result<()> {
    if is_safe_pkg_token(&id.id) {
        Ok(())
    } else {
        Err(crate::Error::Message(format!(
            "refusing unsafe package id: {}",
            id.id
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::{InstallState, Package, PackageSource};

    fn pkg(source: PackageSource, id: &str, name: &str) -> Package {
        Package::stub(source, id, name, "", "1", InstallState::Available)
    }

    #[test]
    fn firefox_matches_across_sources() {
        let flat = pkg(PackageSource::Flatpak, "org.mozilla.firefox", "Firefox");
        let pac = pkg(PackageSource::Pacman, "firefox", "firefox");
        assert!(packages_match(&flat, &pac));
        assert_eq!(search_text_for(&flat).to_lowercase(), "firefox");
    }

    #[test]
    fn rust_does_not_match_rustdesk() {
        let a = pkg(PackageSource::Pacman, "rust", "rust");
        let b = pkg(PackageSource::Aur, "rustdesk", "RustDesk");
        assert!(!packages_match(&a, &b));
    }

    #[test]
    fn code_matches_vscodium_alias() {
        let a = pkg(PackageSource::Pacman, "code", "code");
        let b = pkg(PackageSource::Flatpak, "com.vscodium.codium", "VSCodium");
        assert!(packages_match(&a, &b));
    }

    #[test]
    fn safe_tokens() {
        assert!(is_safe_pkg_token("firefox"));
        assert!(is_safe_pkg_token("org.mozilla.firefox"));
        assert!(!is_safe_pkg_token("foo;rm"));
        assert!(!is_safe_pkg_token(""));
    }

    #[test]
    fn normalize_strips_bin_suffix() {
        assert_eq!(
            normalize_app_key("spotify-bin", "spotify-bin"),
            normalize_app_key("spotify", "spotify")
        );
    }
}
