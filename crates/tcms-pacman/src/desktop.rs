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
    resolve_packages(&mut apps).await;
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

async fn resolve_packages(apps: &mut [DesktopApp]) {
    use futures_util::stream::{self, StreamExt};

    let paths: Vec<(usize, String)> = apps
        .iter()
        .enumerate()
        .filter(|(_, app)| !app.is_flatpak)
        .map(|(idx, app)| (idx, app.desktop_path.to_string_lossy().into_owned()))
        .collect();

    let results: Vec<(usize, Option<(String, Option<String>)>)> = stream::iter(paths)
        .map(|(idx, path)| async move {
            let out = match run("pacman", ["-Qo", &path]).await {
                Ok(o) => o,
                Err(_) => return (idx, None),
            };
            if !out.success() {
                return (idx, None);
            }
            let owned = out
                .stdout
                .trim()
                .split("owned by ")
                .nth(1)
                .and_then(|rest| {
                    let mut parts = rest.split_whitespace();
                    let name = parts.next()?.to_string();
                    let version = parts.next().map(str::to_string);
                    Some((name, version))
                });
            (idx, owned)
        })
        .buffer_unordered(16)
        .collect()
        .await;

    for (idx, owned) in results {
        if let Some((name, version)) = owned {
            apps[idx].package_name = Some(name);
            apps[idx].version = version;
        }
    }
}
