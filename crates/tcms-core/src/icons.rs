//! Download and cache remote application icons.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::error::{Error, Result};

const MAX_ICON_BYTES: u64 = 2 * 1024 * 1024; // 2 MiB

pub fn icon_cache_dir() -> Result<PathBuf> {
    let base = dirs::cache_dir().ok_or_else(|| Error::Config("no cache dir".into()))?;
    let dir = base.join("the-cursed-moon-store").join("icons");
    fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn cache_key(url: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    url.hash(&mut h);
    let ext = extension_for_url(url);
    format!("{:x}.{ext}", h.finish())
}

fn extension_for_url(url: &str) -> &'static str {
    let lower = url.to_ascii_lowercase();
    let path = lower.split('?').next().unwrap_or(&lower);
    if path.ends_with(".svg") {
        "svg"
    } else if path.ends_with(".webp") {
        "webp"
    } else if path.ends_with(".jpg") || path.ends_with(".jpeg") {
        "jpg"
    } else if path.ends_with(".gif") {
        "gif"
    } else {
        "png"
    }
}

fn looks_like_image(bytes: &[u8], content_type: Option<&str>) -> bool {
    if let Some(ct) = content_type {
        let ct = ct.to_ascii_lowercase();
        if ct.contains("image/") {
            return true;
        }
        if ct.contains("text/") || ct.contains("json") || ct.contains("html") {
            return false;
        }
    }
    matches!(
        bytes,
        [0x89, b'P', b'N', b'G', ..]
            | [0xFF, 0xD8, 0xFF, ..]
            | [b'G', b'I', b'F', b'8', ..]
            | [b'R', b'I', b'F', b'F', ..]
            | [b'<', b's', b'v', b'g', ..]
            | [b'<', b'?', b'x', b'm', b'l', ..]
    ) || bytes.starts_with(b"<svg")
        || (bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP")
}

/// Return a local cached path for `url`, downloading when missing.
pub async fn ensure_cached_icon(url: &str) -> Result<PathBuf> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(Error::Message("icon URL must be http(s)".into()));
    }
    let dir = icon_cache_dir()?;
    let path = dir.join(cache_key(url));
    if path.exists() && path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(path);
    }
    let client = reqwest::Client::builder()
        .user_agent("TheCursedMoonStore/0.1")
        .timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| Error::Message(e.to_string()))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| Error::Message(format!("icon download failed: {e}")))?
        .error_for_status()
        .map_err(|e| Error::Message(format!("icon HTTP error: {e}")))?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    if let Some(len) = response.content_length() {
        if len > MAX_ICON_BYTES {
            return Err(Error::Message(format!(
                "icon too large ({len} bytes, max {MAX_ICON_BYTES})"
            )));
        }
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|e| Error::Message(e.to_string()))?;
    if bytes.len() as u64 > MAX_ICON_BYTES {
        return Err(Error::Message(format!(
            "icon too large ({} bytes, max {MAX_ICON_BYTES})",
            bytes.len()
        )));
    }
    if bytes.is_empty() || !looks_like_image(&bytes, content_type.as_deref()) {
        return Err(Error::Message(
            "downloaded icon is not a valid image".into(),
        ));
    }
    let tmp = path.with_extension("part");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &path)?;
    Ok(path)
}

pub fn cached_icon_path_if_exists(url: &str) -> Option<PathBuf> {
    let dir = icon_cache_dir().ok()?;
    let path = dir.join(cache_key(url));
    path.exists().then_some(path)
}

pub fn is_remote_icon(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}

pub fn is_local_icon_path(value: &str) -> bool {
    Path::new(value).is_absolute() || value.starts_with('/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_detection() {
        assert_eq!(extension_for_url("https://a/b/icon.SVG?x=1"), "svg");
        assert_eq!(extension_for_url("https://a/b/icon.png"), "png");
        assert_eq!(extension_for_url("https://a/b/icon"), "png");
    }

    #[test]
    fn image_magic() {
        assert!(looks_like_image(
            b"\x89PNG\r\n\x1a\n....",
            Some("image/png")
        ));
        assert!(looks_like_image(b"<svg xmlns=", None));
        assert!(!looks_like_image(
            b"{\"error\":true}",
            Some("application/json")
        ));
    }

    #[test]
    fn cache_key_stable() {
        assert_eq!(
            cache_key("https://example.com/a.png"),
            cache_key("https://example.com/a.png")
        );
        assert_ne!(
            cache_key("https://example.com/a.png"),
            cache_key("https://example.com/b.png")
        );
    }
}
