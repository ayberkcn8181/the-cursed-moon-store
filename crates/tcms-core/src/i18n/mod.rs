//! Internationalization for The Cursed Moon Store.

mod catalog;

use std::sync::RwLock;

use serde::{Deserialize, Serialize};

/// Supported UI languages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    English,
    Turkish,
    Russian,
    French,
    Korean,
    Japanese,
    Chinese,
    Portuguese,
    Italian,
}

impl Language {
    pub const ALL: [Language; 9] = [
        Self::English,
        Self::Turkish,
        Self::Russian,
        Self::French,
        Self::Korean,
        Self::Japanese,
        Self::Chinese,
        Self::Portuguese,
        Self::Italian,
    ];

    pub fn code(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Turkish => "tr",
            Self::Russian => "ru",
            Self::French => "fr",
            Self::Korean => "ko",
            Self::Japanese => "ja",
            Self::Chinese => "zh",
            Self::Portuguese => "pt",
            Self::Italian => "it",
        }
    }

    /// Native name shown in the language picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::Turkish => "Türkçe",
            Self::Russian => "Русский",
            Self::French => "Français",
            Self::Korean => "한국어",
            Self::Japanese => "日本語",
            Self::Chinese => "中文",
            Self::Portuguese => "Português",
            Self::Italian => "Italiano",
        }
    }

    pub fn from_code(code: &str) -> Option<Self> {
        let code = code.trim().to_ascii_lowercase();
        let primary = code.split(['_', '-', '.']).next().unwrap_or(&code);
        match primary {
            "en" => Some(Self::English),
            "tr" => Some(Self::Turkish),
            "ru" => Some(Self::Russian),
            "fr" => Some(Self::French),
            "ko" => Some(Self::Korean),
            "ja" => Some(Self::Japanese),
            "zh" => Some(Self::Chinese),
            "pt" => Some(Self::Portuguese),
            "it" => Some(Self::Italian),
            _ => None,
        }
    }

    /// Detect from LANG / LC_ALL / LC_MESSAGES. Falls back to English.
    pub fn from_system() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(key) {
                if val.is_empty() || val == "C" || val == "POSIX" {
                    continue;
                }
                if let Some(lang) = Self::from_code(&val) {
                    return lang;
                }
            }
        }
        Self::English
    }
}

impl Default for Language {
    fn default() -> Self {
        Self::from_system()
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.native_name())
    }
}

static CURRENT: RwLock<Language> = RwLock::new(Language::English);

pub fn current() -> Language {
    *CURRENT.read().unwrap_or_else(|e| e.into_inner())
}

pub fn set_current(lang: Language) {
    if let Ok(mut guard) = CURRENT.write() {
        *guard = lang;
    }
}

/// Resolve config language value into a concrete supported language.
/// Unknown values fall back to English. `"system"` uses the OS locale.
pub fn resolve(config_language: &str) -> Language {
    let trimmed = config_language.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("system") {
        return Language::from_system();
    }
    Language::from_code(trimmed).unwrap_or(Language::English)
}

/// Translate a key using the current language (English fallback).
pub fn t(key: &str) -> String {
    t_for(current(), key)
}

/// Translate with simple `{name}` placeholders.
pub fn t_args(key: &str, args: &[(&str, &str)]) -> String {
    let mut text = t(key);
    for (name, value) in args {
        text = text.replace(&format!("{{{name}}}"), value);
    }
    text
}

pub fn t_for(lang: Language, key: &str) -> String {
    catalog::lookup(lang, key)
        .or_else(|| catalog::lookup(Language::English, key))
        .unwrap_or(key)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_cover_english_keys() {
        let en: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(include_str!("../../i18n/en.json")).unwrap();
        for (code, raw) in [
            ("tr", include_str!("../../i18n/tr.json")),
            ("ru", include_str!("../../i18n/ru.json")),
            ("fr", include_str!("../../i18n/fr.json")),
            ("ko", include_str!("../../i18n/ko.json")),
            ("ja", include_str!("../../i18n/ja.json")),
            ("zh", include_str!("../../i18n/zh.json")),
            ("pt", include_str!("../../i18n/pt.json")),
            ("it", include_str!("../../i18n/it.json")),
        ] {
            let map: serde_json::Map<String, serde_json::Value> =
                serde_json::from_str(raw).unwrap_or_else(|e| panic!("{code}: {e}"));
            for key in en.keys() {
                assert!(map.contains_key(key), "{code} missing key {key}");
            }
        }
    }

    #[test]
    fn resolve_unknown_falls_back_to_english() {
        assert_eq!(resolve("xx"), Language::English);
        assert_eq!(resolve("tr"), Language::Turkish);
    }

    #[test]
    fn t_args_replaces_placeholders() {
        set_current(Language::English);
        let s = t_args("toast.busy", &[("name", "Firefox")]);
        assert!(s.contains("Firefox"));
        assert!(!s.contains("{name}"));
    }
}
