//! Bounded screenshot downloads from Flathub's media CDN.
use crate::{Error, Result};
use sha2::{Digest, Sha256};
use std::{path::PathBuf, time::Duration};
const LIMIT: usize = 8 * 1024 * 1024;
pub fn allowed_url(url: &str) -> bool {
    reqwest::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str() == Some("dl.flathub.org")
            && u.username().is_empty()
            && u.password().is_none()
            && u.port().is_none()
            && u.path().starts_with("/media/")
    })
}
pub async fn cached(url: &str) -> Result<PathBuf> {
    if !allowed_url(url) {
        return Err(Error::Message("Unsupported screenshot URL".into()));
    }
    let path = dirs::cache_dir()
        .ok_or_else(|| Error::Config("no cache directory".into()))?
        .join("the-cursed-moon-store/screenshots")
        .join(format!("{:x}", Sha256::digest(url.as_bytes())));
    if path
        .metadata()
        .is_ok_and(|m| m.len() > 0 && m.len() <= LIMIT as u64)
    {
        return Ok(path);
    }
    static DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = DOWNLOADS
        .acquire()
        .await
        .map_err(|e| Error::Message(e.to_string()))?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| Error::Message(e.to_string()))?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Message(e.to_string()))?
        .error_for_status()
        .map_err(|e| Error::Message(e.to_string()))?;
    if response.content_length().is_some_and(|n| n > LIMIT as u64) {
        return Err(Error::Message("Screenshot exceeds 8 MiB".into()));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|e| Error::Message(e.to_string()))?
    {
        if bytes.len() + chunk.len() > LIMIT {
            return Err(Error::Message("Screenshot exceeds 8 MiB".into()));
        }
        bytes.extend_from_slice(&chunk);
    }
    if !(bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes.starts_with(b"\xff\xd8\xff")
        || (bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP")))
    {
        return Err(Error::Message("Unsupported screenshot image".into()));
    }
    crate::atomic_file::write(&path, &bytes)?;
    Ok(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn media_downloads_stay_on_the_https_cdn() {
        assert!(allowed_url("https://dl.flathub.org/media/app/screen.png"));
        for u in [
            "http://dl.flathub.org/media/a",
            "https://dl.flathub.org.evil.test/media/a",
            "https://user@dl.flathub.org/media/a",
            "file:///tmp/a",
            "https://dl.flathub.org/other/a",
            "https://127.0.0.1/media/a",
        ] {
            assert!(!allowed_url(u));
        }
    }
}
