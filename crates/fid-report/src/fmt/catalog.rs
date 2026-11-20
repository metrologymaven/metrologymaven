//! Fixed texts of value display as catalog message keys with an English fallback
//! (`I18N.CATALOG` publishes the translations; this module only reads them).

use std::collections::BTreeMap;

/// Every message key value display uses, with its English fallback text.
pub const MESSAGES: &[(&str, &str)] = &[
    ("report.label.claimed", "claimed"),
    ("report.label.declared_input", "rests on declared input"),
    ("report.label.gauge", "gauge"),
    ("report.label.transcribed", "transcribed"),
    ("report.label.uncharacterized_input", "rests on uncharacterized input"),
    ("report.marker.nonconformance", "nonconforming"),
    ("report.marker.tie", "rounded toward the violated limit at maximum precision"),
    ("report.marker.undecidable_side", "side of the limit undecidable"),
    ("report.outcome.conforms", "conforms"),
    ("report.outcome.indeterminate", "indeterminate"),
    ("report.outcome.nonconforms", "nonconforms"),
    ("report.outcome.not_decidable", "not decidable"),
    ("report.uec.not_evaluated", "not evaluated"),
];

/// Most bytes of one translated text.
pub const MAX_TEXT_BYTES: usize = 512;

/// Why a catalog was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogError {
    /// A key that value display does not use.
    UnknownKey(String),
    /// The same key twice.
    DuplicateKey(String),
    /// An empty text, one longer than [`MAX_TEXT_BYTES`], or one with a control character.
    InvalidText(String),
}

/// Translated texts for the keys in [`MESSAGES`]; a key without a translation falls back to
/// English.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    texts: BTreeMap<String, String>,
}

impl Catalog {
    /// The English catalog: no translations, every key falls back.
    pub fn english() -> Catalog {
        Catalog::default()
    }

    /// A catalog with the given translations. Unknown or duplicate keys and bad texts are
    /// refused, so a typo can never hide a text.
    pub fn new(entries: Vec<(String, String)>) -> Result<Catalog, CatalogError> {
        let mut texts = BTreeMap::new();
        for (k, v) in entries {
            if !MESSAGES.iter().any(|(key, _)| *key == k) {
                return Err(CatalogError::UnknownKey(k));
            }
            if v.is_empty() || v.len() > MAX_TEXT_BYTES || v.chars().any(char::is_control) {
                return Err(CatalogError::InvalidText(k));
            }
            if texts.contains_key(&k) {
                return Err(CatalogError::DuplicateKey(k));
            }
            texts.insert(k, v);
        }
        Ok(Catalog { texts })
    }

    /// The text of `key`: the translation, else the English fallback, else the key itself.
    pub fn text<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(t) = self.texts.get(key) {
            return t;
        }
        MESSAGES.iter().find(|(k, _)| *k == key).map_or(key, |(_, v)| v)
    }
}
