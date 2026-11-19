//! The `rdl-dataset/v1` record: types, bounded parsing, validation in field order, the
//! RFC 8785 canonical form and the dataset digest (REQ-UNC.RDL.FORMAT-001, -002, -005).
//!
//! Every format choice is pinned in `spec/unc/rdl/format/NODE.md` (Design). A record can
//! only be built by [`RdlDataset::from_json`] or [`RdlDataset::from_value`], so every
//! [`RdlDataset`] is valid and carries the digest of its own canonical form.

use crate::error::{internal, invalid, RdlError};
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;

/// The schema tag every record carries in its `schema` field.
pub const SCHEMA: &str = "rdl-dataset/v1";
/// Largest record accepted, in bytes (4 MiB).
pub const MAX_RECORD_BYTES: usize = 4 * 1024 * 1024;
/// Deepest nesting accepted; every array and object counts, the record itself is depth 1.
pub const MAX_DEPTH: usize = 32;
/// Most inputs per record.
pub const MAX_INPUTS: usize = 256;
/// Most reference outputs per record.
pub const MAX_OUTPUTS: usize = 256;
/// Most cross-check entries per record.
pub const MAX_CROSS_CHECKS: usize = 16;
/// Longest free text (provenance source and method), in bytes.
pub const MAX_TEXT: usize = 512;
/// Longest name (role, output name, case, oracle, generator version), in bytes.
pub const MAX_NAME: usize = 64;
/// Longest aim or generator ID, in bytes.
pub const MAX_AIM: usize = 128;
/// Longest decimal reference value, in bytes.
pub const MAX_DECIMAL: usize = 128;
/// Largest `precision_digits`.
pub const MAX_PRECISION_DIGITS: u32 = 1000;

/// Dataset tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Tier {
    /// External (NMI) data.
    X,
    /// Generated data with high-precision references.
    G,
    /// Open suites.
    O,
    /// Blind sets.
    B,
}

impl Tier {
    /// Every tier.
    pub const ALL: [Tier; 4] = [Tier::X, Tier::G, Tier::O, Tier::B];

    /// Wire form: one upper-case letter.
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::X => "X",
            Tier::G => "G",
            Tier::O => "O",
            Tier::B => "B",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.as_str() == s)
    }
}

/// Unit of an input blob. The declared conversion to SI is [`Unit::si_unit`] and
/// [`Unit::si_factor`]; the conversion itself happens where the input is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unit {
    M,
    Mm,
    /// Micrometre, spelled `µm` with U+00B5 MICRO SIGN.
    Um,
    Rad,
    Deg,
}

impl Unit {
    /// Every unit.
    pub const ALL: [Unit; 5] = [Unit::M, Unit::Mm, Unit::Um, Unit::Rad, Unit::Deg];

    /// Wire form. `µm` is U+00B5 followed by `m`; `um` and U+03BC are refused.
    pub fn as_str(self) -> &'static str {
        match self {
            Unit::M => "m",
            Unit::Mm => "mm",
            Unit::Um => "\u{00b5}m",
            Unit::Rad => "rad",
            Unit::Deg => "deg",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<Unit> {
        Unit::ALL.into_iter().find(|u| u.as_str() == s)
    }

    /// The SI unit the input converts to.
    pub fn si_unit(self) -> &'static str {
        match self {
            Unit::M | Unit::Mm | Unit::Um => "m",
            Unit::Rad | Unit::Deg => "rad",
        }
    }

    /// The declared conversion factor to [`Unit::si_unit`], as exact text.
    pub fn si_factor(self) -> &'static str {
        match self {
            Unit::M | Unit::Rad => "1",
            Unit::Mm => "1/1000",
            Unit::Um => "1/1000000",
            Unit::Deg => "pi/180",
        }
    }
}

/// Quantity kind of a reference output. Values are in SI: metres, radians, or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    Length,
    Angle,
    Dimensionless,
}

impl Kind {
    /// Every kind.
    pub const ALL: [Kind; 3] = [Kind::Length, Kind::Angle, Kind::Dimensionless];

    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Length => "length",
            Kind::Angle => "angle",
            Kind::Dimensionless => "dimensionless",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<Kind> {
        Kind::ALL.into_iter().find(|k| k.as_str() == s)
    }
}

/// Who may see the record's content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Visibility {
    Public,
    ReferenceOnly,
    Blind,
}

impl Visibility {
    /// Every visibility.
    pub const ALL: [Visibility; 3] = [Visibility::Public, Visibility::ReferenceOnly, Visibility::Blind];

    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Public => "public",
            Visibility::ReferenceOnly => "reference_only",
            Visibility::Blind => "blind",
        }
    }

    /// Parses the wire form exactly.
    pub fn parse(s: &str) -> Option<Visibility> {
        Visibility::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// An IEEE 754 binary64 value held as its bits. Wire form `0x` followed by exactly 16
/// lower-case hex digits; NaN and infinities are refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HexF64(u64);

impl HexF64 {
    /// The raw bits.
    pub fn bits(self) -> u64 {
        self.0
    }

    /// The value.
    pub fn value(self) -> f64 {
        f64::from_bits(self.0)
    }

    /// Wire form.
    pub fn to_wire(self) -> String {
        format!("0x{:016x}", self.0)
    }

    fn parse(s: &str) -> Option<HexF64> {
        let digits = s.strip_prefix("0x")?;
        if digits.len() != 16 || !digits.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        u64::from_str_radix(digits, 16).ok().map(HexF64)
    }

    fn is_finite(self) -> bool {
        (self.0 >> 52) & 0x7ff != 0x7ff
    }

    fn sign_clear(self) -> bool {
        self.0 >> 63 == 0
    }
}

/// An input: a SHA-256 reference to a blob, its unit and its role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    pub digest: Digest,
    pub unit: Unit,
    pub role: String,
}

/// One reference output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RefOutput {
    pub name: String,
    pub kind: Kind,
    pub value_hex: HexF64,
    /// The value as a decimal with more digits, when the reference has them.
    pub value_dec: Option<String>,
    /// Test uncertainty, finite with the sign bit clear.
    pub test_uncertainty_hex: HexF64,
}

/// Reference outputs and the precision they were computed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    pub outputs: Vec<RefOutput>,
    pub precision_digits: u32,
}

/// The generator that made a tier G record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorRecord {
    pub id: String,
    pub version: String,
    /// Decimal u64 seed without leading zeros, as text so it survives JSON exactly.
    pub seed: String,
}

/// Outcome of one independent-oracle cross-check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrossOutcome {
    Agree,
    Disagree,
}

impl CrossOutcome {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            CrossOutcome::Agree => "agree",
            CrossOutcome::Disagree => "disagree",
        }
    }
}

/// One cross-check record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossCheck {
    pub oracle: String,
    pub outcome: CrossOutcome,
    pub max_deviation_hex: HexF64,
}

/// Where the record came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// Source reference (citation, URL or generator description). Kept in the public view.
    pub source: String,
    pub method: Option<String>,
}

/// One validated RDL dataset record with its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RdlDataset {
    id: String,
    tier: Tier,
    aim: String,
    inputs: Vec<BlobRef>,
    reference: Reference,
    generator: Option<GeneratorRecord>,
    cross_check: Vec<CrossCheck>,
    provenance: Provenance,
    licence: String,
    visibility: Visibility,
    digest: Digest,
}

// ------------------------------------------------------------------ grammars

fn name_ok(s: &str, max: usize) -> bool {
    let mut b = s.bytes();
    s.len() <= max
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Whether `s` is a kernel aim: an algorithm ID without version, `module.feature.method`,
/// each segment `[a-z][a-z0-9_]*`, at most [`MAX_AIM`] bytes (KRN.MAN.CARD grammar).
pub fn aim_ok(s: &str) -> bool {
    let segs: Vec<&str> = s.split('.').collect();
    s.len() <= MAX_AIM && segs.len() == 3 && segs.iter().all(|x| name_ok(x, MAX_AIM))
}

fn case_ok(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_NAME
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase() || c.is_ascii_digit())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
}

/// Splits a dataset ID `rdl:<tier>/<aim>/<case>/<nnnn>` into tier and aim, or `None`.
pub fn parse_id(s: &str) -> Option<(Tier, &str)> {
    let rest = s.strip_prefix("rdl:")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let [tier, aim, case, number] = parts.as_slice() else { return None };
    let tier = Tier::parse(tier)?;
    let number_ok = number.len() == 4 && number.bytes().all(|c| c.is_ascii_digit());
    (aim_ok(aim) && case_ok(case) && number_ok).then_some((tier, *aim))
}

fn generator_id_ok(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_AIM
        && matches!(b.next(), Some(c) if c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'_' | b'.' | b'-'))
}

fn version_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_NAME
        && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

fn seed_ok(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|c| c.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
        && s.parse::<u64>().is_ok()
}

fn digits(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    i
}

/// `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, at most [`MAX_DECIMAL`] bytes.
fn decimal_ok(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > MAX_DECIMAL {
        return false;
    }
    let mut i = usize::from(b[0] == b'-');
    let start = i;
    i = digits(b, i);
    if i == start || (b[start] == b'0' && i - start > 1) {
        return false;
    }
    if i < b.len() && b[i] == b'.' {
        let f = i + 1;
        i = digits(b, f);
        if i == f {
            return false;
        }
    }
    if i < b.len() && (b[i] == b'e' || b[i] == b'E') {
        i += 1;
        if i < b.len() && (b[i] == b'+' || b[i] == b'-') {
            i += 1;
        }
        let e = i;
        i = digits(b, e);
        if i == e {
            return false;
        }
    }
    i == b.len()
}

/// A single SPDX licence ID (`[A-Za-z0-9][A-Za-z0-9.+-]*`) or `LicenseRef-[A-Za-z0-9.-]+`,
/// at most 64 bytes. Licence expressions (`AND`, `OR`, `WITH`) are refused.
pub fn licence_ok(s: &str) -> bool {
    if s.len() > MAX_NAME {
        return false;
    }
    if let Some(r) = s.strip_prefix("LicenseRef-") {
        return !r.is_empty() && r.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-'));
    }
    let mut b = s.bytes();
    matches!(b.next(), Some(c) if c.is_ascii_alphanumeric())
        && b.all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'+' | b'-'))
}

/// Licences that permit redistribution: the `[policy] redistributable` list of
/// `datasets.toml` (ECO.DATA.CAT). Every other licence, and every `LicenseRef-*`, is treated
/// as not permitting redistribution.
pub const REDISTRIBUTABLE: [&str; 8] =
    ["CC0-1.0", "CC-BY-4.0", "CC-BY-SA-4.0", "MIT", "Apache-2.0", "BSD-3-Clause", "ODbL-1.0", "PDDL-1.0"];

/// Whether `licence` permits redistribution (exact, case-sensitive match).
pub fn licence_redistributable(licence: &str) -> bool {
    REDISTRIBUTABLE.contains(&licence)
}

// ------------------------------------------------------------------ field readers

type Map = BTreeMap<String, CValue>;

fn path(parent: &str, key: &str) -> String {
    if parent.is_empty() {
        key.to_string()
    } else {
        format!("{parent}.{key}")
    }
}

fn object<'a>(v: &'a CValue, field: &str) -> Result<&'a Map, RdlError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(invalid(field, "must be an object")),
    }
}

/// Refuses keys outside `allowed`. The error names the enclosing object, never the key.
fn known_keys(o: &Map, allowed: &[&str], field: &str) -> Result<(), RdlError> {
    if o.keys().all(|k| allowed.contains(&k.as_str())) {
        Ok(())
    } else {
        Err(invalid(if field.is_empty() { "record" } else { field }, "unknown key"))
    }
}

/// A present, non-null value.
fn present<'a>(o: &'a Map, key: &str) -> Option<&'a CValue> {
    o.get(key).filter(|v| !matches!(v, CValue::Null))
}

fn required<'a>(o: &'a Map, parent: &str, key: &str) -> Result<&'a CValue, RdlError> {
    present(o, key).ok_or_else(|| invalid(path(parent, key), "required"))
}

/// An optional field: absent is `None`, `null` is refused (absent is the only spelling).
fn optional<'a>(o: &'a Map, parent: &str, key: &str) -> Result<Option<&'a CValue>, RdlError> {
    match o.get(key) {
        None => Ok(None),
        Some(CValue::Null) => Err(invalid(path(parent, key), "omit the field instead of null")),
        Some(v) => Ok(Some(v)),
    }
}

fn string<'a>(v: &'a CValue, field: &str) -> Result<&'a str, RdlError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(invalid(field, "must be a string")),
    }
}

/// Unicode general category Cf (format characters), Unicode 16.0, as inclusive ranges.
/// Bidi overrides and isolates (U+202A..U+202E, U+2066..U+2069), zero-width characters and
/// tag characters can make text display differently from its bytes, so free text refuses
/// them. `fixtures/rdl/format/make_fixtures.py` checks this table against Python's
/// `unicodedata`.
pub const FORMAT_CHARS: [(u32, u32); 21] = [
    (0x00ad, 0x00ad),
    (0x0600, 0x0605),
    (0x061c, 0x061c),
    (0x06dd, 0x06dd),
    (0x070f, 0x070f),
    (0x0890, 0x0891),
    (0x08e2, 0x08e2),
    (0x180e, 0x180e),
    (0x200b, 0x200f),
    (0x202a, 0x202e),
    (0x2060, 0x2064),
    (0x2066, 0x206f),
    (0xfeff, 0xfeff),
    (0xfff9, 0xfffb),
    (0x110bd, 0x110bd),
    (0x110cd, 0x110cd),
    (0x13430, 0x1343f),
    (0x1bca0, 0x1bca3),
    (0x1d173, 0x1d17a),
    (0xe0001, 0xe0001),
    (0xe0020, 0xe007f),
];

/// Whether `c` is a control (Cc) or format (Cf) character.
pub fn is_control_or_format(c: char) -> bool {
    let u = u32::from(c);
    c.is_control() || FORMAT_CHARS.iter().any(|&(lo, hi)| lo <= u && u <= hi)
}

/// Printable text: 1..=`max` bytes, no control (Cc) or format (Cf) characters.
fn text(v: &CValue, field: &str, max: usize) -> Result<String, RdlError> {
    let s = string(v, field)?;
    if s.is_empty() || s.len() > max || s.chars().any(is_control_or_format) {
        return Err(invalid(field, "must be 1 to the maximum bytes of printable text"));
    }
    Ok(s.to_string())
}

fn grammar(v: &CValue, field: &str, ok: fn(&str) -> bool, reason: &'static str) -> Result<String, RdlError> {
    let s = string(v, field)?;
    if ok(s) {
        Ok(s.to_string())
    } else {
        Err(invalid(field, reason))
    }
}

fn array<'a>(v: &'a CValue, field: &str, min: usize, max: usize) -> Result<&'a [CValue], RdlError> {
    match v {
        CValue::Arr(a) if a.len() < min => Err(invalid(field, "too few entries")),
        CValue::Arr(a) if a.len() > max => Err(invalid(field, "too many entries")),
        CValue::Arr(a) => Ok(a),
        _ => Err(invalid(field, "must be an array")),
    }
}

fn hex(v: &CValue, field: &str, non_negative: bool) -> Result<HexF64, RdlError> {
    let h = HexF64::parse(string(v, field)?).ok_or_else(|| invalid(field, "not 0x plus 16 lower-case hex digits"))?;
    if !h.is_finite() {
        return Err(invalid(field, "not finite"));
    }
    if non_negative && !h.sign_clear() {
        return Err(invalid(field, "sign bit set"));
    }
    Ok(h)
}

// ------------------------------------------------------------------ record parsing

fn parse_input(v: &CValue, field: &str) -> Result<BlobRef, RdlError> {
    let o = object(v, field)?;
    let d = path(field, "digest");
    let digest = Digest::parse(string(required(o, field, "digest")?, &d)?)
        .map_err(|_| invalid(d.as_str(), "not sha256: plus 64 lower-case hex digits"))?;
    let u = path(field, "unit");
    let unit = Unit::parse(string(required(o, field, "unit")?, &u)?).ok_or(RdlError::BadUnit { field: u })?;
    let r = path(field, "role");
    let role = grammar(required(o, field, "role")?, &r, |s| name_ok(s, MAX_NAME), "not [a-z][a-z0-9_]*")?;
    known_keys(o, &["digest", "role", "unit"], field)?;
    Ok(BlobRef { digest, unit, role })
}

fn parse_output(v: &CValue, field: &str) -> Result<RefOutput, RdlError> {
    let o = object(v, field)?;
    let n = path(field, "name");
    let name = grammar(required(o, field, "name")?, &n, |s| name_ok(s, MAX_NAME), "not [a-z][a-z0-9_]*")?;
    let k = path(field, "kind");
    let kind =
        Kind::parse(string(required(o, field, "kind")?, &k)?).ok_or_else(|| invalid(k, "not a known quantity kind"))?;
    let value_hex = hex(required(o, field, "value_hex")?, &path(field, "value_hex"), false)?;
    let value_dec = match optional(o, field, "value_dec")? {
        None => None,
        Some(v) => Some(grammar(v, &path(field, "value_dec"), decimal_ok, "not a decimal number")?),
    };
    let tu = path(field, "test_uncertainty_hex");
    let test_uncertainty_hex = match present(o, "test_uncertainty_hex") {
        None => return Err(RdlError::MissingTestUncertainty { field: tu }),
        Some(CValue::Str(s)) if s.is_empty() => return Err(RdlError::MissingTestUncertainty { field: tu }),
        Some(v) => hex(v, &tu, true)?,
    };
    known_keys(o, &["kind", "name", "test_uncertainty_hex", "value_dec", "value_hex"], field)?;
    Ok(RefOutput { name, kind, value_hex, value_dec, test_uncertainty_hex })
}

fn parse_reference(v: &CValue) -> Result<Reference, RdlError> {
    let field = "reference";
    let o = object(v, field)?;
    let mut outputs: Vec<RefOutput> = Vec::new();
    for (i, item) in array(required(o, field, "outputs")?, "reference.outputs", 1, MAX_OUTPUTS)?.iter().enumerate() {
        let out = parse_output(item, &format!("reference.outputs.{i}"))?;
        if outputs.iter().any(|p| p.name == out.name) {
            return Err(invalid(format!("reference.outputs.{i}.name"), "duplicate output name"));
        }
        outputs.push(out);
    }
    let precision_digits = match required(o, field, "precision_digits")? {
        CValue::Num(n) if *n >= 1.0 && *n <= f64::from(MAX_PRECISION_DIGITS) && n.fract() == 0.0 => *n as u32,
        _ => return Err(invalid("reference.precision_digits", "not an integer from 1 to 1000")),
    };
    known_keys(o, &["outputs", "precision_digits"], field)?;
    Ok(Reference { outputs, precision_digits })
}

fn parse_generator(v: &CValue) -> Result<GeneratorRecord, RdlError> {
    let field = "generator";
    let o = object(v, field)?;
    let id = grammar(required(o, field, "id")?, "generator.id", generator_id_ok, "not [a-z][a-z0-9_.-]*")?;
    let version = grammar(required(o, field, "version")?, "generator.version", version_ok, "not a version")?;
    let seed = grammar(required(o, field, "seed")?, "generator.seed", seed_ok, "not a decimal u64")?;
    known_keys(o, &["id", "seed", "version"], field)?;
    Ok(GeneratorRecord { id, version, seed })
}

fn parse_cross_check(v: &CValue, field: &str) -> Result<CrossCheck, RdlError> {
    let o = object(v, field)?;
    let oracle =
        grammar(required(o, field, "oracle")?, &path(field, "oracle"), generator_id_ok, "not [a-z][a-z0-9_.-]*")?;
    let oc = path(field, "outcome");
    let outcome = match string(required(o, field, "outcome")?, &oc)? {
        "agree" => CrossOutcome::Agree,
        "disagree" => CrossOutcome::Disagree,
        _ => return Err(invalid(oc, "not agree or disagree")),
    };
    let max_deviation_hex = hex(required(o, field, "max_deviation_hex")?, &path(field, "max_deviation_hex"), true)?;
    known_keys(o, &["max_deviation_hex", "oracle", "outcome"], field)?;
    Ok(CrossCheck { oracle, outcome, max_deviation_hex })
}

fn parse_provenance(o: &Map) -> Result<Provenance, RdlError> {
    let missing = |f: &str| RdlError::MissingProvenance { field: f.to_string() };
    let p = object(present(o, "provenance").ok_or_else(|| missing("provenance"))?, "provenance")?;
    let source = match present(p, "source") {
        None => return Err(missing("provenance.source")),
        Some(CValue::Str(s)) if s.is_empty() => return Err(missing("provenance.source")),
        Some(v) => text(v, "provenance.source", MAX_TEXT)?,
    };
    let method = match optional(p, "provenance", "method")? {
        None => None,
        Some(v) => Some(text(v, "provenance.method", MAX_TEXT)?),
    };
    known_keys(p, &["method", "source"], "provenance")?;
    Ok(Provenance { source, method })
}

fn parse_licence(o: &Map) -> Result<String, RdlError> {
    match present(o, "licence") {
        None => Err(RdlError::MissingLicence),
        Some(CValue::Str(s)) if s.is_empty() => Err(RdlError::MissingLicence),
        Some(v) => grammar(v, "licence", licence_ok, "not a single SPDX ID or LicenseRef"),
    }
}

impl RdlDataset {
    /// Parses and validates one record: at most [`MAX_RECORD_BYTES`] bytes, nesting at most
    /// [`MAX_DEPTH`], duplicate keys refused, then [`RdlDataset::from_value`].
    pub fn from_json(bytes: &[u8]) -> Result<RdlDataset, RdlError> {
        if bytes.len() > MAX_RECORD_BYTES {
            return Err(invalid("record", "larger than 4 MiB"));
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_RECORD_BYTES, MAX_DEPTH).map_err(|e| match e {
            CanonError::InputLimit => invalid("record", "nested deeper than 32"),
            CanonError::DuplicateKey(_) => invalid("record", "duplicate key"),
            _ => invalid("record", "not acceptable JSON"),
        })?;
        RdlDataset::from_value(&v)
    }

    /// Validates a parsed record in field order (`schema`, `id`, `tier`, `aim`, `inputs`,
    /// `reference`, `generator`, `cross_check`, `provenance`, `licence`, `visibility`), then
    /// unknown keys, then the cross-field rules (ID agrees with tier and aim; tier G has a
    /// generator; blind if and only if tier B; `public` only with a redistributable licence).
    /// The first failure is returned.
    pub fn from_value(v: &CValue) -> Result<RdlDataset, RdlError> {
        let o = object(v, "record")?;
        if string(required(o, "", "schema")?, "schema")? != SCHEMA {
            return Err(invalid("schema", "not rdl-dataset/v1"));
        }
        let id = string(required(o, "", "id")?, "id")?.to_string();
        let parsed = parse_id(&id).ok_or_else(|| RdlError::BadId { field: "id".into() })?;
        let tier = Tier::parse(string(required(o, "", "tier")?, "tier")?)
            .ok_or_else(|| invalid("tier", "not X, G, O or B"))?;
        let aim = grammar(required(o, "", "aim")?, "aim", aim_ok, "not module.feature.method")?;
        let mut inputs = Vec::new();
        for (i, item) in array(required(o, "", "inputs")?, "inputs", 1, MAX_INPUTS)?.iter().enumerate() {
            inputs.push(parse_input(item, &format!("inputs.{i}"))?);
        }
        let reference = parse_reference(required(o, "", "reference")?)?;
        let generator = optional(o, "", "generator")?.map(parse_generator).transpose()?;
        let mut cross_check = Vec::new();
        for (i, item) in array(required(o, "", "cross_check")?, "cross_check", 0, MAX_CROSS_CHECKS)?.iter().enumerate()
        {
            cross_check.push(parse_cross_check(item, &format!("cross_check.{i}"))?);
        }
        let provenance = parse_provenance(o)?;
        let licence = parse_licence(o)?;
        let visibility = Visibility::parse(string(required(o, "", "visibility")?, "visibility")?)
            .ok_or_else(|| invalid("visibility", "not public, reference_only or blind"))?;
        known_keys(
            o,
            &[
                "aim",
                "cross_check",
                "generator",
                "id",
                "inputs",
                "licence",
                "provenance",
                "reference",
                "schema",
                "tier",
                "visibility",
            ],
            "",
        )?;
        if parsed != (tier, aim.as_str()) {
            return Err(RdlError::BadId { field: "id".into() });
        }
        if tier == Tier::G && generator.is_none() {
            return Err(invalid("generator", "required for tier G"));
        }
        if (tier == Tier::B) != (visibility == Visibility::Blind) {
            return Err(invalid("visibility", "blind if and only if tier B"));
        }
        if visibility == Visibility::Public && !licence_redistributable(&licence) {
            return Err(invalid("visibility", "licence does not permit redistribution"));
        }
        let mut rec = RdlDataset {
            id,
            tier,
            aim,
            inputs,
            reference,
            generator,
            cross_check,
            provenance,
            licence,
            visibility,
            digest: Digest::TOMBSTONE,
        };
        let d = fid_canon::digest(&rec.to_value()).map_err(|_| internal("canonical encoding of the record"))?;
        rec.digest = Digest::from_bytes(d.0);
        Ok(rec)
    }

    /// The record as JSON. Optional fields that are `None` are absent. Its RFC 8785 bytes
    /// are [`RdlDataset::canonical_bytes`] and their SHA-256 is [`RdlDataset::digest`].
    pub fn to_value(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_string());
        let inputs = self
            .inputs
            .iter()
            .map(|i| obj([("digest", s(i.digest.as_str())), ("role", s(&i.role)), ("unit", s(i.unit.as_str()))]))
            .collect();
        let outputs = self
            .reference
            .outputs
            .iter()
            .map(|r| {
                let mut m = BTreeMap::new();
                m.insert("kind".to_string(), s(r.kind.as_str()));
                m.insert("name".to_string(), s(&r.name));
                m.insert("test_uncertainty_hex".to_string(), s(&r.test_uncertainty_hex.to_wire()));
                if let Some(d) = &r.value_dec {
                    m.insert("value_dec".to_string(), s(d));
                }
                m.insert("value_hex".to_string(), s(&r.value_hex.to_wire()));
                CValue::Obj(m)
            })
            .collect();
        let reference = obj([
            ("outputs", CValue::Arr(outputs)),
            ("precision_digits", CValue::Num(f64::from(self.reference.precision_digits))),
        ]);
        let cross = self
            .cross_check
            .iter()
            .map(|c| {
                obj([
                    ("max_deviation_hex", s(&c.max_deviation_hex.to_wire())),
                    ("oracle", s(&c.oracle)),
                    ("outcome", s(c.outcome.as_str())),
                ])
            })
            .collect();
        let mut prov = BTreeMap::new();
        if let Some(m) = &self.provenance.method {
            prov.insert("method".to_string(), s(m));
        }
        prov.insert("source".to_string(), s(&self.provenance.source));
        let mut m = BTreeMap::new();
        m.insert("aim".to_string(), s(&self.aim));
        m.insert("cross_check".to_string(), CValue::Arr(cross));
        if let Some(g) = &self.generator {
            m.insert(
                "generator".to_string(),
                obj([("id", s(&g.id)), ("seed", s(&g.seed)), ("version", s(&g.version))]),
            );
        }
        m.insert("id".to_string(), s(&self.id));
        m.insert("inputs".to_string(), CValue::Arr(inputs));
        m.insert("licence".to_string(), s(&self.licence));
        m.insert("provenance".to_string(), CValue::Obj(prov));
        m.insert("reference".to_string(), reference);
        m.insert("schema".to_string(), s(SCHEMA));
        m.insert("tier".to_string(), s(self.tier.as_str()));
        m.insert("visibility".to_string(), s(self.visibility.as_str()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical bytes of [`RdlDataset::to_value`] (via `fid-canon`). Encoding
    /// cannot fail for a validated record (it holds only strings and small integers); if it
    /// ever did, the result is `internal_error`, never empty bytes.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, RdlError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| internal("canonical encoding of the record"))
    }

    /// The dataset digest: SHA-256 over the canonical record, which includes the input blob
    /// digests (REQ-002).
    pub fn digest(&self) -> Digest {
        self.digest
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn tier(&self) -> Tier {
        self.tier
    }

    pub fn aim(&self) -> &str {
        &self.aim
    }

    pub fn inputs(&self) -> &[BlobRef] {
        &self.inputs
    }

    pub fn reference(&self) -> &Reference {
        &self.reference
    }

    pub fn generator(&self) -> Option<&GeneratorRecord> {
        self.generator.as_ref()
    }

    pub fn cross_check(&self) -> &[CrossCheck] {
        &self.cross_check
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn licence(&self) -> &str {
        &self.licence
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }
}

pub(crate) fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
