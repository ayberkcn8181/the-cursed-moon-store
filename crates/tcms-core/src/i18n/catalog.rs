//! Translation catalogs loaded from embedded JSON.

use std::collections::HashMap;
use std::sync::LazyLock;

use super::Language;

type Catalog = HashMap<String, String>;

fn parse(json: &str) -> Catalog {
    serde_json::from_str(json).unwrap_or_default()
}

static EN: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/en.json")));
static TR: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/tr.json")));
static RU: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/ru.json")));
static FR: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/fr.json")));
static KO: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/ko.json")));
static JA: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/ja.json")));
static ZH: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/zh.json")));
static PT: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/pt.json")));
static IT: LazyLock<Catalog> = LazyLock::new(|| parse(include_str!("../../i18n/it.json")));

fn catalog(lang: Language) -> &'static Catalog {
    match lang {
        Language::English => &EN,
        Language::Turkish => &TR,
        Language::Russian => &RU,
        Language::French => &FR,
        Language::Korean => &KO,
        Language::Japanese => &JA,
        Language::Chinese => &ZH,
        Language::Portuguese => &PT,
        Language::Italian => &IT,
    }
}

pub fn lookup(lang: Language, key: &str) -> Option<&'static str> {
    catalog(lang).get(key).map(|s| s.as_str())
}
