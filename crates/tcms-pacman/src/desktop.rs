//! Discover installed GUI applications via desktop entries.

use std::path::{Path, PathBuf};

use tcms_core::process::run;
use tcms_core::Result;
use tokio::fs;

#[derive(Debug, Clone)]
pub struct DesktopApp {
    #[allow(dead_code)]
    pub desktop_id: String,
    pub name: String,
    pub comment: Option<String>,
    pub icon: Option<String>,
    pub categories: Vec<String>,
    pub package_name: Option<String>,
    pub version: Option<String>,
    pub is_flatpak: bool,
    pub desktop_path: PathBuf,
}

pub async fn discover_desktop_apps() -> Result<Vec<DesktopApp>> {
    let mut dirs = vec![
        PathBuf::from("/usr/share/applications"),
        PathBuf::from("/usr/local/share/applications"),
    ];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/applications"));
    }

    discover_desktop_apps_in(&dirs).await
}

/// Discover entries in explicit roots (also used by isolated CLI integration tests).
pub async fn discover_desktop_apps_in(dirs: &[PathBuf]) -> Result<Vec<DesktopApp>> {
    let mut apps = Vec::new();
    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }
        let mut entries = match fs::read_dir(&dir).await {
            Ok(e) => e,
            Err(_) => continue,
        };
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("desktop") {
                continue;
            }
            if let Ok(Some(app)) = parse_desktop_file(&path).await {
                apps.push(app);
            }
        }
    }

    // Resolve package ownership in one batch where possible.
    resolve_packages(&mut apps).await?;
    Ok(apps)
}

async fn parse_desktop_file(path: &Path) -> Result<Option<DesktopApp>> {
    let text = fs::read_to_string(path).await?;
    let mut in_desktop_entry = false;
    let mut name: Option<String> = None;
    let mut name_en: Option<String> = None;
    let mut comment: Option<String> = None;
    let mut icon: Option<String> = None;
    let mut categories: Vec<String> = Vec::new();
    let mut no_display = false;
    let mut hidden = false;
    let mut app_type = String::new();
    let mut is_flatpak = false;

    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry || line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key {
            "Type" => app_type = value.to_string(),
            "Name" => name = Some(value.to_string()),
            "Name[en]" | "Name[en_US]" => name_en = Some(value.to_string()),
            "Comment" => comment = Some(value.to_string()),
            "Icon" => icon = Some(value.to_string()),
            "Categories" => {
                categories = value
                    .split(';')
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            "NoDisplay" => no_display = value.eq_ignore_ascii_case("true"),
            "Hidden" => hidden = value.eq_ignore_ascii_case("true"),
            "X-Flatpak" => is_flatpak = true,
            _ => {}
        }
    }

    if no_display || hidden || (!app_type.is_empty() && app_type != "Application") {
        return Ok(None);
    }
    // Prefer the unlocalized Name; fall back to English if missing.
    let display_name = name.or(name_en);
    let Some(display_name) = display_name else {
        return Ok(None);
    };

    let desktop_id = path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown.desktop")
        .to_string();

    Ok(Some(DesktopApp {
        desktop_id,
        name: display_name,
        comment,
        icon,
        categories,
        package_name: None,
        version: None,
        is_flatpak,
        desktop_path: path.to_path_buf(),
    }))
}

async fn resolve_packages(apps: &mut [DesktopApp]) -> Result<()> {
    let paths: Vec<String> = apps
        .iter()
        .filter(|app| !app.is_flatpak)
        .map(|app| app.desktop_path.to_string_lossy().into_owned())
        .collect();
    let mut owners = std::collections::HashMap::new();
    // Bound argv size without launching one pacman process per desktop entry.
    for chunk in paths.chunks(128) {
        let mut args = vec!["-Qo", "--"];
        args.extend(chunk.iter().map(String::as_str));
        let out = run("pacman", args).await?;
        // Exit 1 is also used for unowned user-created desktop files. Preserve
        // all successful owners from a mixed batch instead of dropping them.
        let expected_missing = out
            .stderr
            .lines()
            .all(|line| line.starts_with("error: No package owns "));
        if out.status != 0 && (out.status != 1 || !expected_missing) {
            out.ensure_success("pacman -Qo")?;
        }
        owners.extend(parse_owners(&out.stdout));
    }
    for app in apps {
        if let Some((name, version)) = owners.get(app.desktop_path.to_string_lossy().as_ref()) {
            app.package_name = Some(name.clone());
            app.version = Some(version.clone());
        }
    }
    Ok(())
}

fn parse_owners(output: &str) -> std::collections::HashMap<String, (String, String)> {
    output
        .lines()
        .filter_map(|line| {
            let (path, owner) = line.rsplit_once(" is owned by ")?;
            let mut fields = owner.split_whitespace();
            let name = fields.next()?;
            let version = fields.next()?;
            Some((path.to_string(), (name.to_string(), version.to_string())))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_ownership_preserves_paths_and_partial_results() {
        let owners = parse_owners("/usr/share/applications/with spaces.desktop is owned by example 1.2-3\n/usr/share/applications/second.desktop is owned by other 2-1\n");
        assert_eq!(owners.len(), 2);
        assert_eq!(
            owners["/usr/share/applications/with spaces.desktop"],
            ("example".into(), "1.2-3".into())
        );
        assert!(!owners.contains_key("/home/me/.local/share/applications/custom.desktop"));
    }
}
