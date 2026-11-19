//! The pure interpretation resolver (spec node `GDT.RES.RESOLVE`, REQ-GDT.RES-001 .. 004, 008).
//!
//! [`resolve`] maps a [`ResolveInput`] (dialect, characteristic type, modifier set, edition
//! set, defaults-profile choices, optional characteristic override) and a loaded [`DmtTable`]
//! to a [`Resolution`]: `RESOLVED` with the row, the selected variant, its operator chain, the
//! chain digest, the table digest and the row's limitation codes; `UNRESOLVED` with the derived
//! key; or `UNSUPPORTED` with the row. [`get_dmt_row`] returns one row by exact key.
//!
//! **Lookup.** The derived key is `dmt:<dialect>/<characteristic>/<sorted modifiers|none>`
//! (the key grammar of `GDT.RES.SCHEMA` without the row version, which is a property of the
//! row, not of the characteristic). Candidates are the rows whose key is that text followed by
//! `@<v>` (an exact match on every key segment except the version) and whose edition set shares
//! at least one edition with the input. Zero candidates is `UNRESOLVED`; there is no fallback to
//! a near match (another modifier set, another dialect, another edition). Two or more is the
//! error `ambiguous_resolution`: the resolver never picks one (fail closed).
//!
//! **Variant.** An explicit characteristic override first, otherwise the defaults profile's
//! value under the row's key text, otherwise the row's default. The chosen name and its source
//! (`override`, `profile`, `row_default`) are part of the result; a name the row does not have
//! is the error `unknown_variant`, with no chain. A profile row choice (a `dmt:` key) for a row
//! the table does not have is the error `profile_row_unknown`: the profile was written against
//! another table, so none of its choices is trusted.
//!
//! Pure and D0: no I/O, no floating-point arithmetic, no `HashMap`/`HashSet`, bounded work
//! (one ordered range scan of the table), and no panics. No interpretation is encoded here:
//! every chain comes from table data.

use crate::dmt::ident::{is_label, is_modifier, is_name, MAX_KEY_LEN, MAX_MODIFIERS, MAX_NAME_LEN};
use crate::dmt::row::MAX_LABEL_LEN;
use crate::dmt::{Dialect, DmtKey, DmtRow, DmtTable, OperatorChain, RowStatus};
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::fmt;

/// Most editions in one input (a characteristic's governing standards).
pub const MAX_INPUT_EDITIONS: usize = 16;
/// Most defaults-profile entries in one input (the profile key cap of `GDT.CHAR.DPROF`).
pub const MAX_PROFILE_CHOICES: usize = 4096;
/// Longest defaults-profile key, in bytes (the profile key grammar's cap).
pub const MAX_PROFILE_KEY_LEN: usize = 128;
/// Longest defaults-profile value, in Unicode scalar values (the profile text cap).
pub const MAX_PROFILE_VALUE_CHARS: usize = 280;

/// Longest text echoed back inside an error, in bytes.
const MAX_ECHO: usize = 160;

fn echo(s: &str) -> String {
    let mut end = s.len().min(MAX_ECHO);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

/// Where the selected variant came from (REQ-GDT.RES.RESOLVE-005).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VariantSource {
    /// An explicit characteristic override.
    Override,
    /// The defaults profile's choice for the row.
    Profile,
    /// The row's default variant.
    RowDefault,
}

impl VariantSource {
    /// Wire form: `override`, `profile` or `row_default`.
    pub fn as_str(self) -> &'static str {
        match self {
            VariantSource::Override => "override",
            VariantSource::Profile => "profile",
            VariantSource::RowDefault => "row_default",
        }
    }
}

/// Errors of the resolver. [`ResolveError::code`] is the stable code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// `bad_input{reason}`: an input field breaks its grammar or a limit.
    BadInput {
        /// Static reason.
        reason: &'static str,
    },
    /// `unknown_variant{variant}`: the override or the profile names a variant the matching
    /// row does not have (REQ-006).
    UnknownVariant {
        /// The variant name (truncated).
        variant: String,
        /// Who named it: [`VariantSource::Override`] or [`VariantSource::Profile`].
        source: VariantSource,
        /// The row's key.
        row: String,
    },
    /// `ambiguous_resolution{key}`: two or more rows match the derived key and the edition
    /// set; the resolver refuses to choose.
    Ambiguous {
        /// The derived key.
        key: String,
        /// How many rows matched.
        count: usize,
    },
    /// `bad_dmt_key{key}`: `gdt.get_dmt_row@1` was given text that is not a row key.
    BadDmtKey {
        /// The text (truncated).
        key: String,
    },
    /// `dmt_row_unknown{key}`: no row has this key.
    DmtRowUnknown {
        /// The key.
        key: String,
    },
    /// `profile_row_unknown{key}`: the defaults profile holds a row choice (a `dmt:` key) for
    /// a row the loaded table does not have. The profile was written against another table;
    /// its choices are refused rather than silently replaced by row defaults.
    ProfileRowUnknown {
        /// The key (truncated).
        key: String,
    },
    /// `encode_failed`: canonical JSON could not be written (not reachable for valid values).
    Encode,
}

impl ResolveError {
    /// The stable code.
    pub fn code(&self) -> &'static str {
        match self {
            ResolveError::BadInput { .. } => "bad_input",
            ResolveError::UnknownVariant { .. } => "unknown_variant",
            ResolveError::Ambiguous { .. } => "ambiguous_resolution",
            ResolveError::BadDmtKey { .. } => "bad_dmt_key",
            ResolveError::DmtRowUnknown { .. } => "dmt_row_unknown",
            ResolveError::ProfileRowUnknown { .. } => "profile_row_unknown",
            ResolveError::Encode => "encode_failed",
        }
    }

    /// The static reason, where there is one.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            ResolveError::BadInput { reason } => Some(reason),
            _ => None,
        }
    }

    fn bad(reason: &'static str) -> ResolveError {
        ResolveError::BadInput { reason }
    }
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResolveError::BadInput { reason } => write!(f, "{}{{{reason}}}", self.code()),
            ResolveError::UnknownVariant { variant: s, .. }
            | ResolveError::Ambiguous { key: s, .. }
            | ResolveError::BadDmtKey { key: s }
            | ResolveError::DmtRowUnknown { key: s }
            | ResolveError::ProfileRowUnknown { key: s } => write!(f, "{}{{{}}}", self.code(), s.escape_debug()),
            ResolveError::Encode => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for ResolveError {}

/// The resolver's input (REQ-GDT.RES.RESOLVE-001). Fields are private; [`ResolveInput::new`]
/// checks every one, so a value always obeys the grammars and limits below.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveInput {
    dialect: Dialect,
    characteristic: String,
    modifiers: Vec<String>,
    editions: Vec<String>,
    profile: BTreeMap<String, String>,
    variant_override: Option<String>,
}

impl ResolveInput {
    /// Builds an input. Modifiers and editions are sets: they are sorted here, and a repeated
    /// entry is refused rather than merged. Checks, in order, each failing `bad_input{reason}`:
    /// characteristic (`bad_characteristic`: `[a-z][a-z0-9_]{0,31}`); modifiers
    /// (`too_many_modifiers` past 8, `bad_modifier`, `duplicate_modifier`); editions
    /// (`no_editions`, `too_many_editions` past [`MAX_INPUT_EDITIONS`], `bad_edition`,
    /// `duplicate_edition`); profile (`too_many_profile_keys` past [`MAX_PROFILE_CHOICES`],
    /// `bad_profile_key` when empty or over [`MAX_PROFILE_KEY_LEN`] bytes, `bad_profile_value`
    /// over [`MAX_PROFILE_VALUE_CHARS`] characters); override (`bad_override`: not a variant
    /// name).
    pub fn new(
        dialect: Dialect,
        characteristic: &str,
        modifiers: &[&str],
        editions: &[&str],
        profile: BTreeMap<String, String>,
        variant_override: Option<&str>,
    ) -> Result<ResolveInput, ResolveError> {
        if !is_name(characteristic, MAX_NAME_LEN) {
            return Err(ResolveError::bad("bad_characteristic"));
        }
        if modifiers.len() > MAX_MODIFIERS {
            return Err(ResolveError::bad("too_many_modifiers"));
        }
        if !modifiers.iter().all(|m| is_modifier(m)) {
            return Err(ResolveError::bad("bad_modifier"));
        }
        let modifiers = sorted_set(modifiers).ok_or_else(|| ResolveError::bad("duplicate_modifier"))?;
        if editions.is_empty() {
            return Err(ResolveError::bad("no_editions"));
        }
        if editions.len() > MAX_INPUT_EDITIONS {
            return Err(ResolveError::bad("too_many_editions"));
        }
        if !editions.iter().all(|e| is_label(e, MAX_LABEL_LEN)) {
            return Err(ResolveError::bad("bad_edition"));
        }
        let editions = sorted_set(editions).ok_or_else(|| ResolveError::bad("duplicate_edition"))?;
        if profile.len() > MAX_PROFILE_CHOICES {
            return Err(ResolveError::bad("too_many_profile_keys"));
        }
        for (k, v) in &profile {
            if k.is_empty() || k.len() > MAX_PROFILE_KEY_LEN {
                return Err(ResolveError::bad("bad_profile_key"));
            }
            if v.chars().count() > MAX_PROFILE_VALUE_CHARS {
                return Err(ResolveError::bad("bad_profile_value"));
            }
        }
        if let Some(o) = variant_override {
            if !is_name(o, MAX_NAME_LEN) {
                return Err(ResolveError::bad("bad_override"));
            }
        }
        Ok(ResolveInput {
            dialect,
            characteristic: characteristic.to_owned(),
            modifiers,
            editions,
            profile,
            variant_override: variant_override.map(str::to_owned),
        })
    }

    /// Dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Characteristic type.
    pub fn characteristic(&self) -> &str {
        &self.characteristic
    }

    /// Modifier set, ascending by bytes.
    pub fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    /// Edition set, ascending by bytes.
    pub fn editions(&self) -> &[String] {
        &self.editions
    }

    /// Defaults-profile choices: row key text to variant name (other keys are ignored).
    pub fn profile(&self) -> &BTreeMap<String, String> {
        &self.profile
    }

    /// The characteristic's explicit variant override.
    pub fn variant_override(&self) -> Option<&str> {
        self.variant_override.as_deref()
    }

    /// The derived key `dmt:<dialect>/<characteristic>/<modifiers joined by + | none>`
    /// (REQ-GDT.RES.RESOLVE-003): the canonical row key without its `@<v>` row version.
    pub fn derived_key(&self) -> String {
        let mods = if self.modifiers.is_empty() { "none".to_owned() } else { self.modifiers.join("+") };
        format!("dmt:{}/{}/{}", self.dialect.as_str(), self.characteristic, mods)
    }
}

/// Sorts by bytes; `None` when an entry repeats.
fn sorted_set(items: &[&str]) -> Option<Vec<String>> {
    let mut v: Vec<&str> = items.to_vec();
    v.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    if v.windows(2).any(|w| w[0] == w[1]) {
        return None;
    }
    Some(v.into_iter().map(str::to_owned).collect())
}

/// A resolved characteristic (REQ-GDT.RES.RESOLVE-002, 004, 005). Fields are private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    row: DmtKey,
    row_status: RowStatus,
    variant_source: VariantSource,
    chain: OperatorChain,
    dmt_digest: Digest,
    limitations: Vec<String>,
}

impl Resolved {
    /// The matching row's key (its row ID).
    pub fn row(&self) -> &DmtKey {
        &self.row
    }

    /// The row's status: `supported` or `partial`.
    pub fn row_status(&self) -> RowStatus {
        self.row_status
    }

    /// The selected variant's name.
    pub fn variant(&self) -> &str {
        self.chain.variant()
    }

    /// Where the variant choice came from.
    pub fn variant_source(&self) -> VariantSource {
        self.variant_source
    }

    /// The selected variant's operator chain.
    pub fn chain(&self) -> &OperatorChain {
        &self.chain
    }

    /// The chain digest: SHA-256 over the chain's RFC 8785 canonical JSON.
    pub fn chain_digest(&self) -> Digest {
        self.chain.digest()
    }

    /// The table digest.
    pub fn dmt_digest(&self) -> Digest {
        self.dmt_digest
    }

    /// The row's limitation codes, ascending (non-empty for a `partial` row in practice).
    pub fn limitations(&self) -> &[String] {
        &self.limitations
    }
}

/// No row matches (REQ-GDT.RES.RESOLVE-003). Fields are private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolved {
    key: String,
    dmt_digest: Digest,
}

impl Unresolved {
    /// The derived key that matched no row.
    pub fn key(&self) -> &str {
        &self.key
    }

    /// The digest of the table searched.
    pub fn dmt_digest(&self) -> Digest {
        self.dmt_digest
    }
}

/// The matching row is `unsupported` (REQ-GDT.RES.RESOLVE-004). Fields are private.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported {
    row: DmtKey,
    dmt_digest: Digest,
}

impl Unsupported {
    /// The row's key.
    pub fn row(&self) -> &DmtKey {
        &self.row
    }

    /// The table digest.
    pub fn dmt_digest(&self) -> Digest {
        self.dmt_digest
    }
}

/// The outcome of [`resolve`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// `RESOLVED`.
    Resolved(Resolved),
    /// `UNRESOLVED`.
    Unresolved(Unresolved),
    /// `UNSUPPORTED`.
    Unsupported(Unsupported),
}

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

impl Resolution {
    /// Wire name of the outcome: `RESOLVED`, `UNRESOLVED` or `UNSUPPORTED`.
    pub fn outcome(&self) -> &'static str {
        match self {
            Resolution::Resolved(_) => "RESOLVED",
            Resolution::Unresolved(_) => "UNRESOLVED",
            Resolution::Unsupported(_) => "UNSUPPORTED",
        }
    }

    /// The `--json` / MCP result of `gdt.resolve_chain@1`:
    ///
    /// ```json
    /// { "outcome": "RESOLVED", "row": "dmt:..@1", "row_status": "supported",
    ///   "variant": "default", "variant_source": "row_default", "chain": { ..chain.. },
    ///   "chain_digest": "sha256:..", "dmt_digest": "sha256:..", "limitations": [] }
    /// { "outcome": "UNRESOLVED", "key": "dmt:asme/flatness/none", "dmt_digest": "sha256:.." }
    /// { "outcome": "UNSUPPORTED", "row": "dmt:..@1", "dmt_digest": "sha256:.." }
    /// ```
    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("outcome".to_owned(), s(self.outcome()));
        match self {
            Resolution::Resolved(r) => {
                m.insert("row".to_owned(), s(r.row.as_str()));
                m.insert("row_status".to_owned(), s(r.row_status.as_str()));
                m.insert("variant".to_owned(), s(r.variant()));
                m.insert("variant_source".to_owned(), s(r.variant_source.as_str()));
                m.insert("chain".to_owned(), r.chain.to_cvalue());
                m.insert("chain_digest".to_owned(), s(r.chain.digest().as_str()));
                m.insert("dmt_digest".to_owned(), s(r.dmt_digest.as_str()));
                m.insert("limitations".to_owned(), CValue::Arr(r.limitations.iter().map(|l| s(l)).collect()));
            }
            Resolution::Unresolved(u) => {
                m.insert("key".to_owned(), s(&u.key));
                m.insert("dmt_digest".to_owned(), s(u.dmt_digest.as_str()));
            }
            Resolution::Unsupported(u) => {
                m.insert("row".to_owned(), s(u.row.as_str()));
                m.insert("dmt_digest".to_owned(), s(u.dmt_digest.as_str()));
            }
        }
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON of [`Resolution::to_cvalue`]: byte-identical on every platform
    /// for identical input (REQ-GDT.RES.RESOLVE-001, AC-6).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ResolveError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue()).map_err(|_| ResolveError::Encode)
    }
}

/// Resolves `input` against `table`. See the module documentation for the rules. Errors, in
/// order: `profile_row_unknown` (a profile key starting `dmt:` names no row of `table`; the
/// first in byte order is reported), `ambiguous_resolution` (two or more rows match),
/// `unknown_variant` (the override or the profile names a variant the row lacks). The status
/// branch runs before variant selection, so an `unsupported` row is `UNSUPPORTED` whatever the
/// override says.
pub fn resolve(input: &ResolveInput, table: &DmtTable) -> Result<Resolution, ResolveError> {
    if let Some(k) = input.profile.keys().find(|k| k.starts_with("dmt:") && table.get(k).is_none()) {
        return Err(ResolveError::ProfileRowUnknown { key: echo(k) });
    }
    let key = input.derived_key();
    let dmt_digest = table.digest();
    let unresolved = |key: String| Ok(Resolution::Unresolved(Unresolved { key, dmt_digest }));
    let prefix = format!("{key}@");
    // `@1` is the least version text in byte order (versions have no leading zero), and all
    // keys sharing the prefix are contiguous, so one range scan from here finds every
    // candidate. A prefix too long to form a valid key matches no row.
    if prefix.len() + 1 > MAX_KEY_LEN {
        return unresolved(key);
    }
    let low = match DmtKey::parse(&format!("{prefix}1")) {
        Ok(k) => k,
        Err(_) => return unresolved(key),
    };
    let mut found: Option<&DmtRow> = None;
    let mut count = 0usize;
    for (k, row) in table.rows().range(low..) {
        if !k.as_str().starts_with(&prefix) {
            break;
        }
        if row.editions().iter().any(|e| input.editions.binary_search_by(|x| x.as_bytes().cmp(e.as_bytes())).is_ok()) {
            count += 1;
            found = found.or(Some(row));
        }
    }
    let row = match (found, count) {
        (None, _) => return unresolved(key),
        (Some(r), 1) => r,
        (Some(_), n) => return Err(ResolveError::Ambiguous { key, count: n }),
    };
    if row.status() == RowStatus::Unsupported {
        return Ok(Resolution::Unsupported(Unsupported { row: row.key().clone(), dmt_digest }));
    }
    let (name, source) = match (&input.variant_override, input.profile.get(row.key().as_str())) {
        (Some(o), _) => (o.as_str(), VariantSource::Override),
        (None, Some(p)) => (p.as_str(), VariantSource::Profile),
        (None, None) => (row.default_variant(), VariantSource::RowDefault),
    };
    let chain = row.variant(name).ok_or_else(|| ResolveError::UnknownVariant {
        variant: echo(name),
        source,
        row: row.key().as_str().to_owned(),
    })?;
    Ok(Resolution::Resolved(Resolved {
        row: row.key().clone(),
        row_status: row.status(),
        variant_source: source,
        chain: chain.clone(),
        dmt_digest,
        limitations: row.limitations().to_vec(),
    }))
}

/// `gdt.get_dmt_row@1` (R0): the row with exactly this key. Fails `bad_dmt_key` when `key` is
/// not a canonical row key and `dmt_row_unknown` when no row has it. The row's canonical JSON
/// is [`DmtRow::canonical_bytes`].
pub fn get_dmt_row<'t>(table: &'t DmtTable, key: &str) -> Result<&'t DmtRow, ResolveError> {
    let k = DmtKey::parse(key).map_err(|_| ResolveError::BadDmtKey { key: echo(key) })?;
    table.rows().get(&k).ok_or_else(|| ResolveError::DmtRowUnknown { key: k.as_str().to_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_are_sorted_and_refuse_repeats() {
        assert_eq!(sorted_set(&["mmc", "cz"]), Some(vec!["cz".to_owned(), "mmc".to_owned()]));
        assert_eq!(sorted_set(&["mmc", "mmc"]), None);
        assert_eq!(sorted_set(&[]), Some(Vec::new()));
    }

    #[test]
    fn echo_truncates_on_a_char_boundary() {
        let long = "é".repeat(100);
        let e = echo(&long);
        assert!(e.len() <= MAX_ECHO && long.starts_with(&e));
    }
}
