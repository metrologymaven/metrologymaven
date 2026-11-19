//! Identifiers and grammars of the DMT schema: the row key `dmt:<dialect>/<characteristic>/
//! <modifiers|none>@<v>`, dialects, table versions, algorithm IDs, operator kinds and the
//! small token grammars every other string field uses.
//!
//! Every grammar is ASCII lower case only, so two values that differ only in case can never
//! both be valid: an upper-case variant is refused outright (fail closed).

use super::DmtError;
use std::cmp::Ordering;
use std::fmt;

/// Longest row key, in bytes. Every valid key is also a valid defaults-profile key
/// (`GDT.CHAR.DPROF`: `[a-z0-9][a-z0-9_.:/+@-]{0,127}`).
pub const MAX_KEY_LEN: usize = 128;
/// Most modifiers in one key.
pub const MAX_MODIFIERS: usize = 8;
/// Longest characteristic name, variant name, parameter name or algorithm ID segment.
pub const MAX_NAME_LEN: usize = 32;
/// Longest modifier.
pub const MAX_MODIFIER_LEN: usize = 16;
/// Longest algorithm ID, in bytes.
pub const MAX_ALG_LEN: usize = 128;

pub(crate) fn lower_alnum(c: u8) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

/// `[a-z][a-z0-9_]{0,max-1}`: characteristic, variant, parameter and algorithm segment names.
pub fn is_name(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    !b.is_empty() && b.len() <= max && b[0].is_ascii_lowercase() && b.iter().all(|&c| lower_alnum(c) || c == b'_')
}

/// `[a-z0-9][a-z0-9_]{0,15}` and not `none`: one modifier.
pub fn is_modifier(s: &str) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= MAX_MODIFIER_LEN
        && lower_alnum(b[0])
        && b.iter().all(|&c| lower_alnum(c) || c == b'_')
        && s != "none"
}

/// A label: 1 to `max` bytes of `[a-z0-9.-]`, alphanumeric at both ends, no two separators
/// in a row (editions such as `y14.5-2018`, designators such as `asme-y14.5`). Same grammar
/// as the defaults-profile edition of `GDT.CHAR.DPROF`.
pub fn is_label(s: &str, max: usize) -> bool {
    let b = s.as_bytes();
    let (first, last) = match (b.first(), b.last()) {
        (Some(&f), Some(&l)) => (f, l),
        _ => return false,
    };
    if b.len() > max || !lower_alnum(first) || !lower_alnum(last) {
        return false;
    }
    let mut prev_sep = false;
    for &c in b {
        let sep = c == b'.' || c == b'-';
        if !(lower_alnum(c) || sep) || (sep && prev_sep) {
            return false;
        }
        prev_sep = sep;
    }
    true
}

/// A decimal `u32` written without sign or leading zero, at least 1.
pub(crate) fn parse_pos_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 10 || b[0] == b'0' || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    s.parse::<u32>().ok()
}

/// A dotted ID with a version: `<seg>(.<seg>)*@<u32>`, each segment a [`is_name`] name
/// (actual-value definition IDs and decision-rule hint IDs).
pub fn is_versioned_id(s: &str, max: usize) -> bool {
    if s.len() > max {
        return false;
    }
    match s.split_once('@') {
        Some((body, v)) => parse_pos_u32(v).is_some() && body.split('.').all(|seg| is_name(seg, MAX_NAME_LEN)),
        None => false,
    }
}

/// GD&T dialect of a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Dialect {
    /// ASME Y14.5 family.
    Asme,
    /// ISO GPS family.
    Iso,
}

impl Dialect {
    /// Wire form: `asme` or `iso`.
    pub fn as_str(self) -> &'static str {
        match self {
            Dialect::Asme => "asme",
            Dialect::Iso => "iso",
        }
    }

    /// Parses the wire form (exact, lower case).
    pub fn parse(s: &str) -> Option<Dialect> {
        match s {
            "asme" => Some(Dialect::Asme),
            "iso" => Some(Dialect::Iso),
            _ => None,
        }
    }
}

/// A row key `dmt:<dialect>/<characteristic>/<modifiers|none>@<v>`. Modifiers are strictly
/// ascending in byte order and joined with `+`; the empty set is written `none`. Keys order
/// by their text, byte by byte.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DmtKey {
    text: String,
    dialect: Dialect,
    characteristic: String,
    modifiers: Vec<String>,
    version: u32,
}

impl DmtKey {
    /// Parses a key. Any departure from the grammar, including modifiers that are not in
    /// strictly ascending byte order, is `bad_dmt_key`.
    pub fn parse(s: &str) -> Result<DmtKey, DmtError> {
        DmtKey::parse_opt(s).ok_or_else(|| DmtError::bad_key(s))
    }

    fn parse_opt(s: &str) -> Option<DmtKey> {
        if s.len() > MAX_KEY_LEN {
            return None;
        }
        let rest = s.strip_prefix("dmt:")?;
        let (body, v) = rest.rsplit_once('@')?;
        let version = parse_pos_u32(v)?;
        let mut parts = body.split('/');
        let dialect = Dialect::parse(parts.next()?)?;
        let characteristic = parts.next()?;
        let mods = parts.next()?;
        if parts.next().is_some() || !is_name(characteristic, MAX_NAME_LEN) {
            return None;
        }
        let modifiers: Vec<String> = if mods == "none" {
            Vec::new()
        } else {
            let list: Vec<&str> = mods.split('+').collect();
            if list.len() > MAX_MODIFIERS || !list.iter().all(|m| is_modifier(m)) {
                return None;
            }
            if !list.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()) {
                return None;
            }
            list.into_iter().map(str::to_owned).collect()
        };
        Some(DmtKey { text: s.to_owned(), dialect, characteristic: characteristic.to_owned(), modifiers, version })
    }

    /// Builds the canonical key from its parts (sorting and checking the modifiers).
    pub fn from_parts(
        dialect: Dialect,
        characteristic: &str,
        modifiers: &[&str],
        version: u32,
    ) -> Result<DmtKey, DmtError> {
        let mut m: Vec<&str> = modifiers.to_vec();
        m.sort_unstable();
        let mods = if m.is_empty() { "none".to_owned() } else { m.join("+") };
        DmtKey::parse(&format!("dmt:{}/{characteristic}/{mods}@{version}", dialect.as_str()))
    }

    /// The key text.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// The dialect.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// The characteristic type.
    pub fn characteristic(&self) -> &str {
        &self.characteristic
    }

    /// The modifiers, ascending (empty for `none`).
    pub fn modifiers(&self) -> &[String] {
        &self.modifiers
    }

    /// The row version after `@`.
    pub fn version(&self) -> u32 {
        self.version
    }
}

impl PartialOrd for DmtKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DmtKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.text.as_bytes().cmp(other.text.as_bytes())
    }
}

impl fmt::Display for DmtKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// A semantic version `x.y.z` (each part `0` or up to nine digits without a leading zero; no
/// pre-release or build suffix). Same grammar as `GDT.CHAR.DPROF`. Fields are private, so every
/// value is in range ([`MAX_SEMVER_PART`]) and its text parses back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SemVer {
    major: u32,
    minor: u32,
    patch: u32,
}

/// Largest version part: nine decimal digits.
pub const MAX_SEMVER_PART: u32 = 999_999_999;

fn semver_part(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 9 || !b.iter().all(u8::is_ascii_digit) || (b.len() > 1 && b[0] == b'0') {
        return None;
    }
    s.parse().ok()
}

impl SemVer {
    /// A version from its parts; `None` when a part exceeds [`MAX_SEMVER_PART`].
    pub fn new(major: u32, minor: u32, patch: u32) -> Option<SemVer> {
        let ok = major <= MAX_SEMVER_PART && minor <= MAX_SEMVER_PART && patch <= MAX_SEMVER_PART;
        ok.then_some(SemVer { major, minor, patch })
    }

    /// Major.
    pub fn major(&self) -> u32 {
        self.major
    }

    /// Minor.
    pub fn minor(&self) -> u32 {
        self.minor
    }

    /// Patch.
    pub fn patch(&self) -> u32 {
        self.patch
    }

    /// Parses `x.y.z`.
    pub fn parse(s: &str) -> Option<SemVer> {
        let mut it = s.split('.');
        let v = SemVer {
            major: semver_part(it.next()?)?,
            minor: semver_part(it.next()?)?,
            patch: semver_part(it.next()?)?,
        };
        if it.next().is_some() {
            return None;
        }
        Some(v)
    }
}

impl fmt::Display for SemVer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// An algorithm ID `<module>.<feature>.<method>@<x.y.z>`: three [`is_name`] segments and a
/// [`SemVer`], at most [`MAX_ALG_LEN`] bytes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AlgId(String);

impl AlgId {
    /// Parses an algorithm ID; anything else is `bad_algorithm_id`.
    pub fn parse(s: &str) -> Result<AlgId, DmtError> {
        let ok = s.len() <= MAX_ALG_LEN
            && match s.split_once('@') {
                Some((body, v)) => {
                    let segs: Vec<&str> = body.split('.').collect();
                    SemVer::parse(v).is_some() && segs.len() == 3 && segs.iter().all(|g| is_name(g, MAX_NAME_LEN))
                }
                None => false,
            };
        if ok {
            Ok(AlgId(s.to_owned()))
        } else {
            Err(DmtError::bad_alg(s))
        }
    }

    /// The ID text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The ISO 17450 operator vocabulary (ISO 17450-1, operator and operation terms;
/// [unverified] exact clause numbers): a closed set. Wire form is the lower-case name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperatorKind {
    /// `partition`.
    Partition,
    /// `extraction`.
    Extraction,
    /// `filtration`.
    Filtration,
    /// `reconstruction`.
    Reconstruction,
    /// `association`.
    Association,
    /// `collection`.
    Collection,
    /// `construction`.
    Construction,
    /// `reduction`.
    Reduction,
    /// `evaluation`.
    Evaluation,
}

impl OperatorKind {
    /// Every kind, in declaration order.
    pub const ALL: [OperatorKind; 9] = [
        OperatorKind::Partition,
        OperatorKind::Extraction,
        OperatorKind::Filtration,
        OperatorKind::Reconstruction,
        OperatorKind::Association,
        OperatorKind::Collection,
        OperatorKind::Construction,
        OperatorKind::Reduction,
        OperatorKind::Evaluation,
    ];

    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            OperatorKind::Partition => "partition",
            OperatorKind::Extraction => "extraction",
            OperatorKind::Filtration => "filtration",
            OperatorKind::Reconstruction => "reconstruction",
            OperatorKind::Association => "association",
            OperatorKind::Collection => "collection",
            OperatorKind::Construction => "construction",
            OperatorKind::Reduction => "reduction",
            OperatorKind::Evaluation => "evaluation",
        }
    }

    /// Parses the wire form (exact, lower case); anything else is `unknown_operator_kind`.
    pub fn parse(s: &str) -> Result<OperatorKind, DmtError> {
        OperatorKind::ALL.into_iter().find(|k| k.as_str() == s).ok_or_else(|| DmtError::unknown_kind(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert!(is_label("y14.5-2018", 32));
        assert!(!is_label("y14..5", 32));
        assert!(!is_label("-y", 32));
        assert!(!is_label("Y14.5", 32));
        assert!(!is_label("", 32));
    }

    #[test]
    fn versioned_ids() {
        assert!(is_versioned_id("av.position.axis@1", 96));
        assert!(!is_versioned_id("av.position.axis@01", 96));
        assert!(!is_versioned_id("av..axis@1", 96));
        assert!(!is_versioned_id("av.axis", 96));
    }

    #[test]
    fn semver() {
        assert_eq!(SemVer::parse("1.20.3").map(|v| v.to_string()), Some("1.20.3".to_owned()));
        assert!(SemVer::parse("1.02.3").is_none());
        assert!(SemVer::parse("1.2").is_none());
        assert!(SemVer::parse("1.2.3-rc1").is_none());
    }
}
