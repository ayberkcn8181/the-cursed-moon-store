use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use sha2::{Digest, Sha256, Sha512};

use crate::archive::{self, Compression};
use crate::model::{LauncherInstallation, ToolRelease};
use crate::safety::safe_component;
use crate::staging::Stage;

const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
const GE_PROTON_RELEASES: &str =
    "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases";
const WINE_GE_RELEASES: &str =
    "https://api.github.com/repos/GloriousEggroll/wine-ge-custom/releases";
const DXVK_RELEASES: &str = "https://api.github.com/repos/doitsujin/dxvk/releases";

pub async fn proton_ge_releases(limit: usize) -> Result<Vec<ToolRelease>> {
    releases(GE_PROTON_RELEASES, ".tar.gz", limit, false).await
}

pub async fn proton_ge_releases_channel(
    limit: usize,
    allow_prerelease: bool,
) -> Result<Vec<ToolRelease>> {
    releases(GE_PROTON_RELEASES, ".tar.gz", limit, allow_prerelease).await
}

pub async fn wine_ge_releases(limit: usize) -> Result<Vec<ToolRelease>> {
    releases(WINE_GE_RELEASES, ".tar.xz", limit, false).await
}

pub async fn wine_ge_releases_channel(
    limit: usize,
    allow_prerelease: bool,
) -> Result<Vec<ToolRelease>> {
    releases(WINE_GE_RELEASES, ".tar.xz", limit, allow_prerelease).await
}

pub async fn dxvk_releases(limit: usize) -> Result<Vec<ToolRelease>> {
    releases(DXVK_RELEASES, ".tar.gz", limit, false).await
}

pub async fn dxvk_releases_channel(
    limit: usize,
    allow_prerelease: bool,
) -> Result<Vec<ToolRelease>> {
    releases(DXVK_RELEASES, ".tar.gz", limit, allow_prerelease).await
}

async fn releases(
    endpoint: &str,
    archive_suffix: &str,
    limit: usize,
    allow_prerelease: bool,
) -> Result<Vec<ToolRelease>> {
    let client = http_client()?;
    let mut releases: Vec<ToolRelease> = client
        .get(endpoint)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    releases.retain(|release| {
        (allow_prerelease || !release.prerelease)
            && release.assets.iter().any(|asset| {
                asset.name.ends_with(archive_suffix)
                    && !asset.name.contains("sha512sum")
                    && asset.size <= MAX_ARTIFACT_BYTES
            })
    });
    releases.truncate(limit);
    Ok(releases)
}

pub async fn install_proton_ge(
    installation: &LauncherInstallation,
    release: &ToolRelease,
) -> Result<PathBuf> {
    install_release(
        &installation.tool_root,
        release,
        ".tar.gz",
        Compression::Gzip,
        "compatibilitytool.vdf",
    )
    .await
}

pub async fn install_wine_ge(
    installation: &LauncherInstallation,
    release: &ToolRelease,
) -> Result<PathBuf> {
    install_release(
        &installation.tool_root,
        release,
        ".tar.xz",
        Compression::Xz,
        "bin/wine",
    )
    .await
}

pub async fn install_dxvk_release(release: &ToolRelease) -> Result<PathBuf> {
    let cache = dirs::cache_dir()
        .context("could not resolve cache directory")?
        .join("the-cursed-moon-store/compatibility/dxvk");
    let existing = cache.join(safe_component(&release.tag_name)?);
    if existing.join("x64/dxgi.dll").is_file() {
        return Ok(existing);
    }
    install_release(
        &cache,
        release,
        ".tar.gz",
        Compression::Gzip,
        "x64/dxgi.dll",
    )
    .await
}

async fn install_release(
    target_root: &Path,
    release: &ToolRelease,
    archive_suffix: &str,
    compression: Compression,
    marker: &str,
) -> Result<PathBuf> {
    let asset = release
        .assets
        .iter()
        .find(|asset| {
            asset.name.ends_with(archive_suffix)
                && !asset.name.contains("sha512sum")
                && asset.size <= MAX_ARTIFACT_BYTES
        })
        .context("release does not contain a supported archive")?;
    validate_github_asset_url(&asset.browser_download_url)?;

    let target_name = safe_component(&release.tag_name)?;
    let stage = Stage::create(target_root)?;
    let archive_path = stage.path.join("artifact");
    download(&asset.browser_download_url, asset.size, &archive_path).await?;
    verify_release_digest(release, asset, &archive_path, &stage.path).await?;

    let extraction_dir = stage.dir.try_clone()?;
    let marker = marker.to_owned();
    let extracted = tokio::task::spawn_blocking(move || {
        archive::extract(&extraction_dir, compression, &marker)
    })
    .await
    .context("archive extraction worker failed")??;
    stage.publish(&extracted, target_name)?;
    Ok(target_root.join(target_name))
}

async fn verify_release_digest(
    release: &ToolRelease,
    asset: &crate::model::ReleaseAsset,
    archive: &Path,
    stage: &Path,
) -> Result<()> {
    if let Some(expected) = asset.digest.as_deref() {
        let Some(expected) = expected.strip_prefix("sha256:") else {
            bail!("unsupported release digest");
        };
        return verify_sha256(archive, expected);
    }
    if let Some(checksum_asset) = release
        .assets
        .iter()
        .find(|candidate| candidate.name.ends_with(".sha512sum"))
    {
        validate_github_asset_url(&checksum_asset.browser_download_url)?;
        let checksum_path = stage.join("artifact.sha512sum");
        download(
            &checksum_asset.browser_download_url,
            checksum_asset.size,
            &checksum_path,
        )
        .await?;
        let result = verify_sha512(archive, &checksum_path);
        let _ = fs::remove_file(checksum_path);
        return result;
    }
    Ok(())
}

fn verify_sha256(archive: &Path, expected: &str) -> Result<()> {
    if expected.len() != 64 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("release does not contain a valid SHA-256 digest");
    }
    let actual = digest_file::<Sha256>(archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("downloaded archive failed SHA-256 verification");
    }
    Ok(())
}

fn verify_sha512(archive: &Path, checksum_file: &Path) -> Result<()> {
    let checksum_text = fs::read_to_string(checksum_file)?;
    let expected = checksum_text
        .split_whitespace()
        .next()
        .context("checksum file is empty")?;
    if expected.len() != 128 || !expected.chars().all(|ch| ch.is_ascii_hexdigit()) {
        bail!("checksum file does not contain a valid SHA-512 digest");
    }
    let actual = digest_file::<Sha512>(archive)?;
    if !actual.eq_ignore_ascii_case(expected) {
        bail!("downloaded archive failed SHA-512 verification");
    }
    Ok(())
}

fn digest_file<D: Digest + Default>(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = D::default();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let bytes = digest.finalize();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut output, "{byte:02x}")?;
    }
    Ok(output)
}

fn http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent("The-Cursed-Moon-Store/0.1")
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(20 * 60))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?)
}

async fn download(url: &str, expected_size: u64, destination: &Path) -> Result<()> {
    if expected_size > MAX_ARTIFACT_BYTES {
        bail!("artifact exceeds the download size limit");
    }
    let response = http_client()?.get(url).send().await?.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|size| size > MAX_ARTIFACT_BYTES)
    {
        bail!("artifact exceeds the download size limit");
    }
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(destination)?;
    let mut downloaded = 0_u64;
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .context("artifact size overflow")?;
        if downloaded > MAX_ARTIFACT_BYTES {
            bail!("artifact exceeds the download size limit");
        }
        file.write_all(&chunk)?;
    }
    file.sync_all()?;
    if expected_size != 0 && downloaded != expected_size {
        bail!("download size mismatch: expected {expected_size}, received {downloaded}");
    }
    Ok(())
}

fn validate_github_asset_url(url: &str) -> Result<()> {
    let parsed = reqwest::Url::parse(url)?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("github.com") {
        bail!("release asset is not hosted on GitHub");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_accepts_https_github_assets() {
        assert!(validate_github_asset_url(
            "https://github.com/example/project/releases/download/v1/file.tar.gz"
        )
        .is_ok());
        assert!(validate_github_asset_url("http://github.com/file").is_err());
        assert!(validate_github_asset_url("https://example.com/file").is_err());
    }
}
