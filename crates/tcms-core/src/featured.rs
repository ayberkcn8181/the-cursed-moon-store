//! Featured / spotlight collections and Flathub app metadata.

use serde::Deserialize;

use crate::package::{license_is_proprietary, InstallState, Package, PackageId, PackageSource};

#[derive(Debug, Clone)]
pub struct FeaturedSection {
    pub id: String,
    pub title_key: String,
    pub packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct FlathubCollection {
    hits: Vec<FlathubHit>,
}

#[derive(Debug, Deserialize)]
struct FlathubHit {
    app_id: Option<String>,
    name: Option<String>,
    summary: Option<String>,
    #[serde(rename = "type")]
    app_type: Option<String>,
    icon: Option<String>,
    developer_name: Option<String>,
    installs_last_month: Option<u64>,
    project_license: Option<String>,
}

#[derive(Debug, Deserialize)]
struct FlathubApp {
    #[serde(alias = "id")]
    app_id: Option<String>,
    name: Option<String>,
    summary: Option<String>,
    description: Option<String>,
    icon: Option<String>,
    developer_name: Option<String>,
    project_license: Option<String>,
    #[serde(default)]
    is_free_license: Option<bool>,
    #[serde(default)]
    urls: FlathubUrls,
    #[serde(default)]
    categories: Vec<String>,
    #[serde(default)]
    screenshots: Vec<FlathubScreenshot>,
    #[serde(default)]
    releases: Vec<FlathubRelease>,
}

#[derive(Debug, Deserialize)]
struct FlathubScreenshot {
    #[serde(default)]
    sizes: Vec<FlathubImage>,
    caption: Option<String>,
}
#[derive(Debug, Deserialize)]
struct FlathubImage {
    src: String,
    width: String,
}
#[derive(Debug, Deserialize)]
struct FlathubRelease {
    version: String,
    date: Option<String>,
    description: Option<String>,
    url: Option<String>,
}
fn screenshots(items: Vec<FlathubScreenshot>) -> Vec<crate::package::Screenshot> {
    items
        .into_iter()
        .take(5)
        .filter_map(|item| {
            let image = item
                .sizes
                .into_iter()
                .filter(|s| crate::screenshots::allowed_url(&s.src))
                .filter_map(|s| Some((s.width.parse::<u32>().ok()?, s)))
                .filter(|(w, _)| (600..=1600).contains(w))
                .min_by_key(|(w, _)| w.abs_diff(1248))?
                .1;
            Some(crate::package::Screenshot {
                url: image.src,
                caption: item.caption,
            })
        })
        .collect()
}
fn releases(items: Vec<FlathubRelease>) -> Vec<crate::package::ReleaseNote> {
    items
        .into_iter()
        .take(5)
        .map(|item| {
            let description = item.description.unwrap_or_default();
            crate::package::ReleaseNote {
                version: item.version,
                date: item.date,
                description: crate::package::strip_simple_html(&description).unwrap_or(description),
                url: item.url.filter(|u| u.starts_with("https://")),
            }
        })
        .collect()
}
#[derive(Debug, Default, Deserialize)]
struct FlathubUrls {
    homepage: Option<String>,
    bugtracker: Option<String>,
    donation: Option<String>,
}

fn http_client() -> crate::Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent("TheCursedMoonStore/0.1")
        .timeout(std::time::Duration::from_secs(12))
        .build()
        .map_err(|e| crate::Error::Message(e.to_string()))
}

/// Fetch Flathub collection hits and map them to packages (apps only).
pub async fn fetch_flathub_collection(
    collection: &str,
    per_page: u32,
) -> crate::Result<Vec<Package>> {
    let url =
        format!("https://flathub.org/api/v2/collection/{collection}?page=1&per_page={per_page}");
    let client = http_client()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(crate::Error::Message(format!(
            "Flathub HTTP {}",
            resp.status()
        )));
    }
    let body: FlathubCollection = resp
        .json()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub parse failed: {e}")))?;

    let mut packages = Vec::new();
    for hit in body.hits {
        let app_type = hit.app_type.unwrap_or_default();
        if !app_type.is_empty() && app_type != "desktop-application" {
            continue;
        }
        let Some(app_id) = hit.app_id.filter(|s| !s.is_empty()) else {
            continue;
        };
        let name = hit
            .name
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| app_id.clone());
        let summary = hit.summary.unwrap_or_default();
        let installs = hit
            .installs_last_month
            .map(|n| crate::i18n::t_args("featured.installs_month", &[("n", &n.to_string())]))
            .unwrap_or_default();
        let summary = if summary.is_empty() {
            installs.clone()
        } else if installs.is_empty() {
            summary
        } else {
            format!("{summary} · {installs}")
        };
        let license = hit.project_license;
        let mut pkg = Package {
            id: PackageId::new(PackageSource::Flatpak, &app_id),
            name,
            summary: summary.clone(),
            description: summary,
            version: String::new(),
            available_version: None,
            icon_name: Some("application-x-executable".into()),
            icon_url: hit.icon,
            desktop_id: None,
            developer: hit.developer_name.clone(),
            publisher: hit.developer_name,
            license: license.clone(),
            homepage: Some(format!("https://flathub.org/apps/{app_id}")),
            bug_url: None,
            donate_url: None,
            permissions: None,
            is_proprietary: license_is_proprietary(license.as_deref()),
            size_bytes: None,
            state: InstallState::Available,
            installed_elsewhere: false,
            foreign_status: None,
            explicitly_installed: None,
            screenshots: Vec::new(),
            releases: Vec::new(),
            categories: vec!["Flatpak".into()],
        };
        pkg.apply_license_heuristics();
        packages.push(pkg);
    }
    Ok(packages)
}

/// Fetch a single Flathub app's rich metadata.
pub async fn fetch_flathub_app(app_id: &str) -> crate::Result<Package> {
    // Flathub moved app details from /api/v2/apps/{id} to /api/v2/appstream/{id}.
    let url = format!("https://flathub.org/api/v2/appstream/{app_id}");
    let client = http_client()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub app request failed: {e}")))?;
    if !resp.status().is_success() {
        return Err(crate::Error::Message(format!(
            "Flathub app HTTP {}",
            resp.status()
        )));
    }
    let body: FlathubApp = resp
        .json()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub app parse failed: {e}")))?;
    let id = body
        .app_id
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| app_id.to_string());
    let name = body
        .name
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| id.clone());
    let summary = body.summary.clone().unwrap_or_default();
    let description = body
        .description
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| summary.clone());
    let license = body.project_license;
    let size_bytes = fetch_flathub_installed_size(&id).await.ok().flatten();
    let mut pkg = Package {
        id: PackageId::new(PackageSource::Flatpak, &id),
        name,
        summary,
        description,
        version: String::new(),
        available_version: None,
        icon_name: Some("application-x-executable".into()),
        icon_url: body.icon,
        desktop_id: None,
        developer: body.developer_name.clone(),
        publisher: body.developer_name,
        license: license.clone(),
        homepage: body
            .urls
            .homepage
            .or_else(|| Some(format!("https://flathub.org/apps/{id}"))),
        bug_url: body.urls.bugtracker,
        donate_url: body.urls.donation,
        permissions: None,
        is_proprietary: body
            .is_free_license
            .map(|is_free| !is_free)
            .or_else(|| license_is_proprietary(license.as_deref())),
        size_bytes,
        state: InstallState::Available,
        installed_elsewhere: false,
        foreign_status: None,
        explicitly_installed: None,
        screenshots: screenshots(body.screenshots),
        releases: releases(body.releases),
        categories: body.categories,
    };
    pkg.apply_license_heuristics();
    Ok(pkg)
}

#[derive(Debug, Deserialize)]
struct FlathubSummary {
    installed_size: Option<u64>,
}

async fn fetch_flathub_installed_size(app_id: &str) -> crate::Result<Option<u64>> {
    let url = format!("https://flathub.org/api/v2/summary/{app_id}");
    let client = http_client()?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub summary request failed: {e}")))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body: FlathubSummary = resp
        .json()
        .await
        .map_err(|e| crate::Error::Message(format!("Flathub summary parse failed: {e}")))?;
    Ok(body.installed_size)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn appstream_details_keep_categories_and_select_cdn_images() {
        let body:FlathubApp=serde_json::from_str(r#"{"id":"org.example.App","categories":["Utility"],"screenshots":[{"caption":"A & B","sizes":[{"width":"112","src":"https://dl.flathub.org/media/app/thumb.png"},{"width":"1248","src":"https://untrusted.example/image.png"},{"width":"752","src":"https://dl.flathub.org/media/app/large.png"}]}],"releases":[{"version":"2","description":"<p>Fix &amp; improve</p>","url":"file:///tmp/not-a-link"}]}"#).unwrap();
        assert_eq!(body.categories, ["Utility"]);
        let images = screenshots(body.screenshots);
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].url, "https://dl.flathub.org/media/app/large.png");
        let notes = releases(body.releases);
        assert_eq!(notes[0].description, "Fix & improve");
        assert!(notes[0].url.is_none());
        let minimal: FlathubApp = serde_json::from_str("{}").unwrap();
        assert!(minimal.screenshots.is_empty() && minimal.releases.is_empty());
    }
}
