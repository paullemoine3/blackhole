//! The interface language of this process: the catalog used by the shell's own texts (toasts,
//! menus, window titles) and sent to every BlackHole page (`ui/i18n.js`). Rules and catalogs are
//! in `blackhole_domain::i18n`.

pub use blackhole_domain::i18n::message;
use blackhole_domain::i18n::{self, Catalog};
use std::sync::RwLock;

static CURRENT: RwLock<&str> = RwLock::new(i18n::DEFAULT);

/// Applies the `language` setting (a code or "system"); returns the language now shown.
pub fn apply(choice: &str) -> &'static str {
    let lang = i18n::resolve(choice, &system_languages());
    if let Ok(mut current) = CURRENT.write() {
        *current = lang;
    }
    lang
}

/// The system's preferred locales, best first (`fr-FR`…). Empty when it cannot tell.
pub fn system_languages() -> Vec<String> {
    let mut all: Vec<String> = sys_locale::get_locales().collect();
    if all.is_empty() {
        all.extend(
            ["LC_ALL", "LC_MESSAGES", "LANG"]
                .iter()
                .filter_map(|v| std::env::var(v).ok()),
        );
    }
    all
}

pub fn lang() -> &'static str {
    CURRENT.read().map_or(i18n::DEFAULT, |l| *l)
}

pub fn catalog() -> &'static Catalog {
    i18n::catalog(lang())
}

/// The text of `key` in the current language.
pub fn t(key: &str) -> String {
    catalog().get(key).to_owned()
}

/// `key` with its `{name}` parameters.
pub fn tf(key: &str, params: &[(&str, &str)]) -> String {
    catalog().format(key, params)
}

/// The plural form of `key` for `n`, with `{n}` and `params`.
pub fn tn(key: &str, n: usize, params: &[(&str, &str)]) -> String {
    catalog().plural(key, n as u64, params)
}

/// A message from another layer (`i18n::message`) or a raw system text, in the current language.
pub fn render(text: &str) -> String {
    catalog().render(text)
}

/// The current catalog as the JSON object pages read (`{lang, s: {key: text}}`), built once per
/// language: about 60 KB, sent to each page when it is created and when the language changes.
pub fn page_json() -> &'static str {
    use std::sync::OnceLock;
    static JSON: [OnceLock<String>; i18n::LANGUAGES.len()] = [OnceLock::new(), OnceLock::new()];
    let lang = lang();
    let slot = i18n::LANGUAGES
        .iter()
        .position(|(c, _)| *c == lang)
        .unwrap_or(0);
    JSON[slot].get_or_init(|| {
        let cat = i18n::catalog(lang);
        serde_json::json!({"lang": lang, "s": cat.merged()})
            .to_string()
            .replace('<', "\\u003c")
    })
}

/// The error page's strings (`error.*`) in the current language, for its JSON configuration.
pub fn error_strings() -> serde_json::Value {
    serde_json::json!({"lang": lang(), "s": catalog().with_prefix("error.")})
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    /// Every key the shell's code and pages use exists in the catalog (both languages have the
    /// same keys, see the domain's tests). Keys are found by their literal spelling:
    /// `t("…")`, `tf("…"`, `tn("…"` in Rust, `bhT('…'`, `bhTn('…'`, `data-i18n*="…"` in pages.
    #[test]
    fn every_used_key_is_in_the_catalog() {
        let en = blackhole_domain::i18n::catalog("en").merged();
        let has = |key: &str| en.contains_key(key) || en.contains_key(&*format!("{key}.other"));
        let mut missing = BTreeSet::new();
        let mut used = 0;
        let dirs = [
            concat!(env!("CARGO_MANIFEST_DIR"), "/src"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/ui"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../infrastructure/src"),
            concat!(env!("CARGO_MANIFEST_DIR"), "/../domain/src"),
        ];
        for dir in dirs {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                if path.ends_with("i18n.rs") || path.ends_with("i18n.js") {
                    continue;
                }
                for key in used_keys(&text) {
                    used += 1;
                    if !has(&key) {
                        missing.insert(format!("{}: {key}", path.display()));
                    }
                }
            }
        }
        assert!(used > 300, "only {used} keys found: the scan is broken");
        assert!(missing.is_empty(), "keys missing from the catalog: {missing:#?}");
    }

    fn used_keys(text: &str) -> Vec<String> {
        const OPENERS: [&str; 13] = [
            "message(\"",
            "ok_or(\"",
            "Err(\"",
            "t(\"",
            "tf(\"",
            "tn(\"",
            "bhT('",
            "bhTn('",
            "data-i18n=\"",
            "data-i18n-title=\"",
            "data-i18n-placeholder=\"",
            "data-i18n-aria=\"",
            "T('",
        ];
        let mut keys = Vec::new();
        for opener in OPENERS {
            let close = opener.chars().last().unwrap();
            let mut rest = text;
            while let Some(at) = rest.find(opener) {
                let before = rest[..at].chars().last();
                rest = &rest[at + opener.len()..];
                // `t("` must be a call of its own, not the end of another name (`print("`).
                if opener.len() <= 5
                    && before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    continue;
                }
                let Some(end) = rest.find(close) else { break };
                let key = &rest[..end];
                // A key ending with '.' is the start of a key built at run time.
                if !key.is_empty()
                    && key.contains('.')
                    && !key.ends_with('.')
                    && key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
                {
                    keys.push(key.to_owned());
                }
            }
        }
        keys
    }
}
