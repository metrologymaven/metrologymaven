//! The defaults-profile document (`fid.defaults-profile/1`), its strict parser, the key
//! vocabulary it is checked against, and its canonical form and digest.
//!
//! ```json
//! { "schema": "fid.defaults-profile/1", "dialect": "asme", "edition": "y14.5-2018",
//!   "version": "1.0.0", "keys": { "<key>": <value>, ... } }
//! ```
//!
//! A value is a string of at most 280 characters, a boolean, an integer in the IEEE-754
//! safe range, or a flat list (at most 256 items) of those. Null, objects, nested lists
//! and non-integer numbers are refused, so the canonical form never depends on float
//! formatting.

use super::ident::{Edition, ProfileId, ProfileRef, SemVer};
use super::DprofError;
use crate::dmt_flow::roster::Dialect;
use fid_canon::CValue;
use fid_gps::dmt::{profile_vocabulary, Dialect as GpsDialect, DmtTable, ProfileKeyRule};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};

/// The `schema` value of a profile document.
pub const PROFILE_SCHEMA: &str = "fid.defaults-profile/1";
/// Largest accepted profile file, in bytes.
pub const MAX_PROFILE_BYTES: usize = 2 * 1024 * 1024;
/// Deepest accepted nesting: the document, `keys`, and one list level.
pub const MAX_PROFILE_DEPTH: usize = 3;
/// Most keys one profile may set.
pub const MAX_KEYS: usize = 4096;
/// Longest key, in bytes.
pub const MAX_KEY_LEN: usize = 128;
/// Longest text value, in Unicode scalar values (the DMT schema's free-text cap).
pub const MAX_TEXT_CHARS: usize = 280;
/// Most items in one list value.
pub const MAX_LIST_LEN: usize = 256;
/// Most keys in one vocabulary.
pub const MAX_VOCABULARY_KEYS: usize = 65_536;
/// Largest integer magnitude accepted (2^53 - 1).
pub const MAX_SAFE_INT: i64 = 9_007_199_254_740_991;

const FIELDS: [&str; 5] = ["dialect", "edition", "keys", "schema", "version"];

/// One profile value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileValue {
    /// A string.
    Text(String),
    /// A boolean.
    Flag(bool),
    /// A safe integer.
    Int(i64),
    /// A flat list of scalars.
    List(Vec<ProfileValue>),
}

/// Whether `k` matches the key grammar `[a-z0-9][a-z0-9_.:/+@-]{0,127}`.
pub fn is_valid_key(k: &str) -> bool {
    let b = k.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_KEY_LEN
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter().all(|&c| {
            c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b':' | b'/' | b'+' | b'@' | b'-')
        })
}

/// What a key's value must be.
#[derive(Debug, Clone, PartialEq, Eq)]
enum ValueRule {
    /// Any profile value (explicit vocabularies built with [`KeyVocabulary::new`]).
    Any,
    /// A text value.
    Text,
    /// A text naming one of these variants of the DMT row with this key.
    Variant(BTreeSet<String>),
}

/// The keys a profile may set, and what each value must be. The real vocabulary is owned by
/// the DMT schema (`GDT.RES.SCHEMA`, `fid_gps::dmt::profile_keys`) and is built with
/// [`KeyVocabulary::from_dmt_table`]; [`KeyVocabulary::new`] builds an explicit key list that
/// accepts any value (tests and tools that check a key subset).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct KeyVocabulary {
    keys: BTreeMap<String, ValueRule>,
    dialect: Option<Dialect>,
}

impl KeyVocabulary {
    /// Builds an explicit vocabulary whose keys accept any value. Fails `bad_vocabulary` on a
    /// key outside the key grammar, a repeated key, or more than [`MAX_VOCABULARY_KEYS`] keys.
    pub fn new<'a>(keys: impl IntoIterator<Item = &'a str>) -> Result<KeyVocabulary, DprofError> {
        let mut map = BTreeMap::new();
        for k in keys {
            KeyVocabulary::add(&mut map, k, ValueRule::Any)?;
        }
        Ok(KeyVocabulary { keys: map, dialect: None })
    }

    /// The DMT schema's vocabulary (`fid.dmt-profile-keys/1`) for profiles of `dialect`
    /// against `table`: the schema's named keys (text values) and the key of every row of
    /// that dialect with two or more variants (value: the name of one of that row's
    /// variants). A profile of another dialect is refused with
    /// `malformed_profile{vocabulary_dialect}`.
    pub fn from_dmt_table(table: &DmtTable, dialect: Dialect) -> Result<KeyVocabulary, DprofError> {
        let gps_dialect = match dialect {
            Dialect::Asme => GpsDialect::Asme,
            Dialect::Iso => GpsDialect::Iso,
        };
        let mut map = BTreeMap::new();
        for (k, rule) in profile_vocabulary(table, gps_dialect) {
            let rule = match rule {
                ProfileKeyRule::Text => ValueRule::Text,
                ProfileKeyRule::Variant(v) => ValueRule::Variant(v.into_iter().collect()),
            };
            KeyVocabulary::add(&mut map, &k, rule)?;
        }
        Ok(KeyVocabulary { keys: map, dialect: Some(dialect) })
    }

    fn add(map: &mut BTreeMap<String, ValueRule>, k: &str, rule: ValueRule) -> Result<(), DprofError> {
        if !is_valid_key(k) {
            return Err(DprofError::BadVocabulary { reason: "bad_key" });
        }
        if map.len() >= MAX_VOCABULARY_KEYS {
            return Err(DprofError::BadVocabulary { reason: "too_many_keys" });
        }
        if map.insert(k.to_owned(), rule).is_some() {
            return Err(DprofError::BadVocabulary { reason: "duplicate_key" });
        }
        Ok(())
    }

    /// Whether `key` is allowed.
    pub fn contains(&self, key: &str) -> bool {
        self.keys.contains_key(key)
    }

    /// The dialect this vocabulary is for, when it comes from the DMT schema.
    pub fn dialect(&self) -> Option<Dialect> {
        self.dialect
    }

    /// Whether `value` is allowed for `key` (false for an unknown key).
    pub fn allows(&self, key: &str, value: &ProfileValue) -> bool {
        match (self.keys.get(key), value) {
            (Some(ValueRule::Any), _) => true,
            (Some(ValueRule::Text), ProfileValue::Text(_)) => true,
            (Some(ValueRule::Variant(names)), ProfileValue::Text(t)) => names.contains(t),
            _ => false,
        }
    }
}

/// A parsed defaults profile. Fields are private: values are made only by the strict
/// parser ([`parse_profile`], [`profile_from_value`]).
///
/// ```compile_fail
/// fn forge(p: &fid_gdt::chars::dprof::DefaultsProfile) -> usize {
///     p.keys.len()
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultsProfile {
    dialect: Dialect,
    edition: Edition,
    version: SemVer,
    keys: BTreeMap<String, ProfileValue>,
}

impl DefaultsProfile {
    /// Dialect, edition and version.
    pub fn id(&self) -> ProfileId {
        ProfileId::new(self.dialect, self.edition.clone(), self.version)
    }

    /// Dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Edition.
    pub fn edition(&self) -> &Edition {
        &self.edition
    }

    /// Semantic version.
    pub fn version(&self) -> SemVer {
        self.version
    }

    /// The default choices, by key.
    pub fn keys(&self) -> &BTreeMap<String, ProfileValue> {
        &self.keys
    }
}

/// A profile together with its canonical bytes and digest. Fields are private: values are
/// made only by the strict parser, and [`ProfileDoc::digest_holds`] re-checks the digest.
///
/// ```compile_fail
/// fn forge(d: fid_gdt::chars::dprof::ProfileDoc) -> fid_gdt::chars::dprof::ProfileDoc {
///     fid_gdt::chars::dprof::ProfileDoc { canonical: Vec::new(), ..d }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileDoc {
    profile: DefaultsProfile,
    canonical: Vec<u8>,
    digest: Digest,
}

impl ProfileDoc {
    /// The full reference `dp:<dialect>-<edition>@<version>#sha256:<hex>`.
    pub fn reference(&self) -> ProfileRef {
        ProfileRef::new(self.profile.id(), self.digest)
    }

    /// The profile.
    pub fn profile(&self) -> &DefaultsProfile {
        &self.profile
    }

    /// RFC 8785 canonical JSON of the whole document.
    pub fn canonical(&self) -> &[u8] {
        &self.canonical
    }

    /// SHA-256 of [`ProfileDoc::canonical`].
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// Whether the digest is the SHA-256 of the canonical bytes.
    pub fn digest_holds(&self) -> bool {
        Digest::from_bytes(fid_canon::digest_bytes(&self.canonical).0) == self.digest
    }
}

fn malformed(reason: &'static str) -> DprofError {
    DprofError::Malformed { reason }
}

fn scalar(v: &CValue) -> Result<ProfileValue, DprofError> {
    match v {
        CValue::Str(s) => {
            if s.chars().count() > MAX_TEXT_CHARS {
                return Err(malformed("text_too_long"));
            }
            Ok(ProfileValue::Text(s.clone()))
        }
        CValue::Bool(b) => Ok(ProfileValue::Flag(*b)),
        CValue::Num(f) => {
            // The source literal was already checked to be an integer literal
            // ([`integer_literals_only`]); this checks its value on the RFC 8785 text of the
            // number, not with float arithmetic: magnitude <= 2^53 - 1.
            let t = fid_canon::format_number(*f).map_err(|_| malformed("bad_number"))?;
            let digits = t.strip_prefix('-').unwrap_or(&t);
            if digits.is_empty() || digits.len() > 16 || !digits.bytes().all(|c| c.is_ascii_digit()) {
                return Err(malformed("bad_number"));
            }
            let n: i64 = t.parse().map_err(|_| malformed("bad_number"))?;
            if !(-MAX_SAFE_INT..=MAX_SAFE_INT).contains(&n) {
                return Err(malformed("bad_number"));
            }
            Ok(ProfileValue::Int(n))
        }
        _ => Err(malformed("bad_value")),
    }
}

fn value(v: &CValue) -> Result<ProfileValue, DprofError> {
    match v {
        CValue::Arr(items) => {
            if items.len() > MAX_LIST_LEN {
                return Err(malformed("list_too_long"));
            }
            items.iter().map(scalar).collect::<Result<Vec<_>, _>>().map(ProfileValue::List)
        }
        other => scalar(other),
    }
}

fn text<'a>(o: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, DprofError> {
    match o.get(k) {
        Some(CValue::Str(s)) => Ok(s),
        Some(_) => Err(malformed("wrong_type")),
        None => Err(malformed("missing_field")),
    }
}

/// Parses a profile from a parsed JSON value. Structure is checked for every key before
/// the vocabulary, so a malformed document fails `malformed_profile` even when it also
/// holds an unknown key; among unknown keys the first in byte order is named. A vocabulary
/// from the DMT schema also refuses, after all structural checks and in this order, a profile
/// of another dialect (`malformed_profile{vocabulary_dialect}`), an unknown key, and a value
/// its key does not allow (`malformed_profile{value_not_allowed}`).
pub fn profile_from_value(v: &CValue, vocab: &KeyVocabulary) -> Result<ProfileDoc, DprofError> {
    let o = match v {
        CValue::Obj(o) => o,
        _ => return Err(malformed("wrong_type")),
    };
    if o.keys().any(|k| !FIELDS.contains(&k.as_str())) {
        return Err(malformed("unknown_field"));
    }
    if text(o, "schema")? != PROFILE_SCHEMA {
        return Err(malformed("bad_schema"));
    }
    let dialect = Dialect::parse(text(o, "dialect")?).ok_or_else(|| malformed("bad_dialect"))?;
    let edition = Edition::parse(text(o, "edition")?).ok_or_else(|| malformed("bad_edition"))?;
    let version = SemVer::parse(text(o, "version")?).ok_or_else(|| malformed("bad_version"))?;
    let raw = match o.get("keys") {
        Some(CValue::Obj(k)) => k,
        Some(_) => return Err(malformed("wrong_type")),
        None => return Err(malformed("missing_field")),
    };
    if raw.len() > MAX_KEYS {
        return Err(malformed("too_many_keys"));
    }
    let mut keys = BTreeMap::new();
    for (k, val) in raw {
        if !is_valid_key(k) {
            return Err(malformed("bad_key"));
        }
        keys.insert(k.clone(), value(val)?);
    }
    // Vocabulary checks start here, after every structural check.
    if vocab.dialect().is_some_and(|d| d != dialect) {
        return Err(malformed("vocabulary_dialect"));
    }
    if let Some(k) = keys.keys().find(|k| !vocab.contains(k)) {
        return Err(DprofError::UnknownKey { key: k.clone() });
    }
    if keys.iter().any(|(k, v)| !vocab.allows(k, v)) {
        return Err(malformed("value_not_allowed"));
    }
    let canonical = fid_canon::to_canonical_bytes(v).map_err(|e| malformed(e.code()))?;
    let digest = Digest::from_bytes(fid_canon::digest_bytes(&canonical).0);
    Ok(ProfileDoc { profile: DefaultsProfile { dialect, edition, version, keys }, canonical, digest })
}

/// Whether every number literal in the JSON text `bytes` is an integer literal (an optional
/// minus and decimal digits: no `.`, no exponent). Runs on the source text, so `1.0`,
/// `1e2` and `7.0000000000000001` are refused even though they parse to integral values.
/// Expects text that already parsed as JSON; string contents are skipped.
pub fn integer_literals_only(bytes: &[u8]) -> bool {
    let mut in_str = false;
    let mut esc = false;
    let mut in_num = false;
    for &b in bytes {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        if in_num {
            match b {
                b'0'..=b'9' => continue,
                b'.' | b'e' | b'E' | b'+' => return false,
                _ => in_num = false,
            }
        }
        match b {
            b'"' => in_str = true,
            b'-' | b'0'..=b'9' => in_num = true,
            _ => {}
        }
    }
    true
}

/// Parses profile file bytes: at most [`MAX_PROFILE_BYTES`], UTF-8 JSON with no duplicate
/// object key and nesting at most [`MAX_PROFILE_DEPTH`], every number literal an integer
/// literal (`malformed_profile{bad_number}`), then [`profile_from_value`]. JSON-level
/// failures are `malformed_profile` with the `fid-canon` code as reason.
pub fn parse_profile(bytes: &[u8], vocab: &KeyVocabulary) -> Result<ProfileDoc, DprofError> {
    if bytes.len() > MAX_PROFILE_BYTES {
        return Err(malformed("too_large"));
    }
    let v =
        fid_canon::parse_with_limits(bytes, MAX_PROFILE_BYTES, MAX_PROFILE_DEPTH).map_err(|e| malformed(e.code()))?;
    if !integer_literals_only(bytes) {
        return Err(malformed("bad_number"));
    }
    profile_from_value(&v, vocab)
}
