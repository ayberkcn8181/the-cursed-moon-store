//! Read-only checks and user-reviewed, locally redacted support reports.
use regex::{Captures, Regex};
use std::sync::LazyLock;

pub fn problem_key(message: &str) -> &'static str {
    let text = message.to_lowercase();
    let has = |terms: &[&str]| terms.iter().any(|term| text.contains(term));
    if has(&[
        "invalid signature",
        "unknown trust",
        "unknown public key",
        "pgp signature",
        "gpgme",
        "geçersiz imza",
    ]) || (text.contains("signature from") && text.contains("invalid"))
    {
        "diagnostics.signature"
    } else if has(&[
        "unable to lock database",
        "could not lock database",
        "db.lck",
        "veritabanı kilitlenemedi",
    ]) {
        "diagnostics.lock"
    } else if has(&[
        "not authorized",
        "not authorised",
        "authentication",
        "authorization",
        "kimlik doğrulama",
    ]) {
        "diagnostics.auth"
    } else if has(&[
        "no space left",
        "not enough free disk",
        "insufficient disk space",
        "aygıt üzerinde boş yer yok",
    ]) {
        "diagnostics.disk"
    } else if has(&[
        "could not resolve",
        "failed to connect",
        "connection timed out",
        "network is unreachable",
        "failed retrieving file",
        "http 404",
        "http 503",
    ]) {
        "diagnostics.network"
    } else if has(&[
        "could not satisfy dependencies",
        "breaks dependency",
        "conflicting dependencies",
        "unresolvable package conflicts",
        "bağımlılıklar sağlanamadı",
    ]) {
        "diagnostics.dependency"
    } else {
        "diagnostics.unknown"
    }
}
pub fn explanation(message: &str) -> String {
    crate::t(problem_key(message))
}

static SECRET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?im)^.*(?:password|passwd|token|secret|authorization|cookie|api[_-]?key)\s*[:=].*$",
    )
    .unwrap()
});
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"https?://[^\s<>\"]+"#).unwrap());
static HOME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"/(?:home|Users)/[^/\s\"']+"#).unwrap());
static EMAIL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"[\w.+-]+@[\w.-]+\.[A-Za-z]{2,}").unwrap());
static IP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b|[0-9a-fA-F:]*:[0-9a-fA-F:]+").unwrap()
});

pub fn redact(
    text: &str,
    home: Option<&str>,
    user: Option<&str>,
    hostname: Option<&str>,
) -> String {
    let mut clean: String = text
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect();
    clean = SECRET
        .replace_all(&clean, "[redacted credential]")
        .into_owned();
    clean = URL
        .replace_all(&clean, |c: &Captures<'_>| {
            reqwest::Url::parse(&c[0])
                .map(|mut url| {
                    let _ = url.set_username("");
                    let _ = url.set_password(None);
                    url.set_query(None);
                    url.set_fragment(None);
                    url.to_string()
                })
                .unwrap_or_else(|_| "[redacted URL]".into())
        })
        .into_owned();
    if let Some(home) = home.filter(|v| v.len() > 1) {
        clean = clean.replace(home, "~");
    }
    clean = HOME.replace_all(&clean, "/home/[user]").into_owned();
    clean = EMAIL.replace_all(&clean, "[email]").into_owned();
    clean = IP
        .replace_all(&clean, |c: &Captures<'_>| {
            if c[0].parse::<std::net::IpAddr>().is_ok() {
                "[ip]".into()
            } else {
                c[0].to_string()
            }
        })
        .into_owned();
    for identity in [user, hostname]
        .into_iter()
        .flatten()
        .filter(|v| !v.is_empty())
    {
        clean = Regex::new(&format!(r"\b{}\b", regex::escape(identity)))
            .unwrap()
            .replace_all(&clean, "[identity]")
            .into_owned();
    }
    clean
}
pub fn redact_local(text: &str) -> String {
    let home = dirs::home_dir();
    let user = std::env::var("USER").ok();
    let hostname = std::fs::read_to_string("/etc/hostname").ok();
    redact(
        text,
        home.as_deref().and_then(|p| p.to_str()),
        user.as_deref(),
        hostname.as_deref().map(str::trim),
    )
}
pub async fn collect(version: &str, config: &crate::AppConfig) -> String {
    let mut report = format!(
        "The Cursed Moon Store {version}\nArchitecture: {}\nSources: pacman={} flatpak={} aur={}\n",
        std::env::consts::ARCH,
        config.enable_pacman,
        config.enable_flatpak,
        config.enable_aur
    );
    if let Ok(os) = std::fs::read_to_string("/etc/os-release") {
        for line in os
            .lines()
            .filter(|l| l.starts_with("ID=") || l.starts_with("VERSION_ID="))
        {
            report.push_str(line);
            report.push('\n');
        }
    }
    let mut pacman = vec!["-Qq".to_string()];
    if !config.advanced.pacman_conf.trim().is_empty() {
        pacman.extend(["--config".into(), config.advanced.pacman_conf.clone()]);
    }
    for (label, program, args, count) in [
        ("pacman installed packages", "pacman", pacman, true),
        (
            "Flatpak user refs",
            "flatpak",
            vec!["list".into(), "--user".into(), "--columns=ref".into()],
            true,
        ),
        (
            "Flatpak system refs",
            "flatpak",
            vec!["list".into(), "--system".into(), "--columns=ref".into()],
            true,
        ),
        ("Polkit launcher", "pkexec", vec!["--version".into()], false),
    ] {
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            crate::run(program, &args),
        )
        .await;
        let value = match result {
            Ok(Ok(out)) if out.success() => {
                if count {
                    out.stdout
                        .lines()
                        .filter(|l| !l.trim().is_empty())
                        .count()
                        .to_string()
                } else {
                    out.stdout.trim().into()
                }
            }
            Ok(Ok(out)) => format!("exit {}: {}", out.status, out.stderr.trim()),
            Ok(Err(error)) => error.to_string(),
            Err(_) => "timed out after 10s".into(),
        };
        report.push_str(&format!("{label}: {value}\n"));
    }
    redact_local(&report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn common_errors_have_distinct_guidance() {
        for (error, key) in [
            ("signature from CachyOS is invalid", "signature"),
            ("unable to lock database", "lock"),
            ("not authorized", "auth"),
            ("No space left on device", "disk"),
            ("Could not resolve host", "network"),
            ("breaks dependency foo", "dependency"),
            ("unknown error", "unknown"),
        ] {
            assert_eq!(problem_key(error), format!("diagnostics.{key}"));
        }
        assert_eq!(
            problem_key("valid signature from publisher"),
            "diagnostics.unknown"
        );
    }
    #[test]
    fn report_preserves_error_but_removes_identifiers_and_credentials() {
        let text = "signature from admin@cachyos.org is invalid\n/home/alice/x /srv/alice/data alice@my-laptop\nhttps://name:password@example.org/log?access=123#secret\nTOKEN=secret-value\nAuthorization: Bearer abc\n192.168.2.1 2001:db8::1";
        let clean = redact(text, Some("/srv/alice"), Some("alice"), Some("my-laptop"));
        for value in [
            "alice",
            "my-laptop",
            "name:password",
            "access=123",
            "#secret",
            "secret-value",
            "Bearer abc",
            "192.168.2.1",
            "2001:db8::1",
            "admin@",
        ] {
            assert!(!clean.contains(value), "{clean}");
        }
        assert!(clean.contains("signature from"));
        assert!(clean.contains("is invalid"));
        assert!(clean.contains("https://example.org/log"));
    }
    #[test]
    fn unicode_and_versions_survive() {
        assert_eq!(
            redact("paket sürümü 0.1.2 12:34", None, None, None),
            "paket sürümü 0.1.2 12:34"
        );
    }
}
