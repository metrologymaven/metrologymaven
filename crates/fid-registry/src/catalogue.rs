//! The `QuantityCatalogue` facade over the unit catalogue. Spec node `CORE.REG.DEF.SCHEMA`
//! (REQ-004, REQ-005).
//!
//! The registry defines no kind, unit or factor (REQ-CORE.REG.DEF-003). A [`Catalogue`] is
//! built either from the tables `fid-geo-frame` publishes ([`Catalogue::geo_frame`], kinds
//! `length` and `angle`) or from a catalogue document in the canonical form below
//! ([`Catalogue::from_json`]), which is how a catalogue with more kinds (`time`,
//! `temperature`, ...) is served once its owner publishes one. Both paths go through the
//! same checks, so a malformed table is refused rather than served.
//!
//! Canonical form (RFC 8785 via `fid-canon`; its SHA-256 is [`QuantityCatalogue::digest`]):
//!
//! ```text
//! {"format":"fid-quantity-catalogue/1",
//!  "kinds":[{"canonical":"m","kind":"length",
//!            "units":[{"den":"1","num":"1","pi":false,"symbol":"m"}, ...]}, ...],
//!  "pi_hex":"<64 lowercase hex digits>"}
//! ```
//!
//! Kinds sorted by name, units by symbol (byte order, all ASCII). A factor is
//! `num / den`, times the catalogue's Π when `pi` is true, with Π = `pi_hex` / 2^254 as
//! `GEO.FRAME.UNITS` pins it. Every choice is pinned in `spec/core/reg/def/schema/NODE.md`.

use crate::validate::{unit_kind, QuantityKinds};
use fid_canon::CValue;
use fid_geo_frame::units::PI_256;
use fid_geo_frame::{AngleUnit, LengthUnit, Unit};
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// The `format` value of a catalogue document.
pub const CATALOGUE_FORMAT: &str = "fid-quantity-catalogue/1";
/// Largest accepted catalogue document, in bytes (1 MiB).
pub const MAX_CATALOGUE_BYTES: usize = 1024 * 1024;
/// Most kinds in one catalogue.
pub const MAX_KINDS: usize = 64;
/// Most units of one kind.
pub const MAX_UNITS_PER_KIND: usize = 64;
/// Longest kind name, in bytes (`^[a-z][a-z0-9_]{0,31}$`).
pub const MAX_KIND_NAME: usize = 32;
/// Longest unit symbol, in bytes (printable ASCII without space).
pub const MAX_SYMBOL: usize = 16;
/// Nesting of a catalogue document: root, `kinds`, kind, `units`, unit.
const CATALOGUE_DEPTH: usize = 5;

/// A positive rational number in lowest terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Rational {
    num: u64,
    den: u64,
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    a
}

impl Rational {
    /// `num / den` when both are positive and already in lowest terms; `None` otherwise, so
    /// one value has exactly one spelling (and one digest).
    pub fn new(num: u64, den: u64) -> Option<Rational> {
        (num > 0 && den > 0 && gcd(num, den) == 1).then_some(Rational { num, den })
    }

    /// `num / den` reduced to lowest terms; `None` when either is zero.
    pub fn reduced(num: u64, den: u64) -> Option<Rational> {
        if num == 0 || den == 0 {
            return None;
        }
        let g = gcd(num, den);
        Some(Rational { num: num / g, den: den / g })
    }

    /// The numerator.
    pub fn num(self) -> u64 {
        self.num
    }

    /// The denominator.
    pub fn den(self) -> u64 {
        self.den
    }

    /// Whether this is exactly 1.
    pub fn is_one(self) -> bool {
        self.num == 1 && self.den == 1
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.num, self.den)
    }
}

/// The exact factor from a unit to its kind's canonical SI unit: `ratio`, times the
/// catalogue's Π when `times_pi` is set (angles in degrees, gon, ...).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Factor {
    pub ratio: Rational,
    pub times_pi: bool,
}

impl Factor {
    /// Whether the unit is the canonical unit of its kind (factor exactly 1).
    pub fn is_one(self) -> bool {
        self.ratio.is_one() && !self.times_pi
    }
}

/// One accepted authoring unit of a kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInfo {
    pub symbol: String,
    pub factor: Factor,
}

/// One quantity kind: its canonical SI unit and every accepted unit (the canonical one
/// included), sorted by symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KindInfo {
    pub name: String,
    pub canonical: String,
    pub units: Vec<UnitInfo>,
}

/// The read-only facade the registry, `CORE.BUS.UNIT` and `CORE.STO.SET` look kinds and
/// units up through (REQ-004, REQ-005).
pub trait QuantityCatalogue {
    /// Every kind name, sorted.
    fn kind_names(&self) -> Vec<&str>;
    /// One kind, or `None` when the catalogue has no such kind (names are case-sensitive).
    fn kind(&self, name: &str) -> Option<&KindInfo>;
    /// Whether `unit` is an accepted unit of `kind`.
    fn accepts(&self, kind: &str, unit: &str) -> bool {
        self.kind(kind).is_some_and(|k| k.units.iter().any(|u| u.symbol == unit))
    }
    /// The factor of a unit (symbols are unique across the catalogue).
    fn factor(&self, unit: &str) -> Option<Factor>;
    /// SHA-256 of the catalogue's canonical JSON form, for the manifest (`CORE.REG.MAN`).
    fn digest(&self) -> Digest;
}

/// Why a catalogue was refused. Messages never echo input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogueError {
    /// The document is larger than [`MAX_CATALOGUE_BYTES`].
    TooLarge,
    /// Malformed JSON, too deep, or an exact duplicate key (`fid-canon` code).
    Json(&'static str),
    /// The document or table does not have the pinned shape.
    Shape(String),
}

impl CatalogueError {
    /// Stable code: `catalogue_too_large`, the `fid-canon` code, or `bad_catalogue`.
    pub fn code(&self) -> &'static str {
        match self {
            CatalogueError::TooLarge => "catalogue_too_large",
            CatalogueError::Json(c) => c,
            CatalogueError::Shape(_) => "bad_catalogue",
        }
    }
}

impl fmt::Display for CatalogueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CatalogueError::Shape(s) => write!(f, "bad_catalogue: {s}"),
            other => f.write_str(other.code()),
        }
    }
}

impl std::error::Error for CatalogueError {}

fn shape(msg: impl Into<String>) -> CatalogueError {
    CatalogueError::Shape(msg.into())
}

/// A checked unit catalogue with its digest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Catalogue {
    kinds: BTreeMap<String, KindInfo>,
    factors: BTreeMap<String, Factor>,
    pi_hex: String,
    digest: Digest,
}

/// Whether `s` is a kind name: `^[a-z][a-z0-9_]{0,31}$`.
pub fn is_kind_name(s: &str) -> bool {
    let mut b = s.bytes();
    s.len() <= MAX_KIND_NAME
        && b.next().is_some_and(|c| c.is_ascii_lowercase())
        && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// Whether `s` is a unit symbol: 1 to 16 bytes of printable ASCII without space.
pub fn is_unit_symbol(s: &str) -> bool {
    !s.is_empty() && s.len() <= MAX_SYMBOL && s.bytes().all(|b| (b'!'..=b'~').contains(&b))
}

impl Catalogue {
    /// The catalogue of `GEO.FRAME.UNITS` as `fid-geo-frame` publishes it: kind names from
    /// [`unit_kind`], symbols from `symbol()`, factors from `ratio()` (lengths) and
    /// `per_half_turn()` (angles: Π / k, or 1 for `rad`), Π from `PI_256`.
    pub fn geo_frame() -> Result<Catalogue, CatalogueError> {
        let mut kinds: BTreeMap<String, Vec<UnitInfo>> = BTreeMap::new();
        let mut add = |unit: Unit, factor: Option<Factor>| -> Result<(), CatalogueError> {
            let factor = factor.ok_or_else(|| shape("fid-geo-frame publishes a factor that is not in lowest terms"))?;
            kinds
                .entry(unit_kind(unit).to_string())
                .or_default()
                .push(UnitInfo { symbol: unit.symbol().to_string(), factor });
            Ok(())
        };
        for u in LengthUnit::ALL {
            let (p, q) = u.ratio();
            add(
                Unit::Length(u),
                Rational::new(u64::from(p), u64::from(q)).map(|ratio| Factor { ratio, times_pi: false }),
            )?;
        }
        for u in AngleUnit::ALL {
            let f = match u.per_half_turn() {
                None => Rational::new(1, 1).map(|ratio| Factor { ratio, times_pi: false }),
                Some(k) => Rational::new(1, u64::from(k)).map(|ratio| Factor { ratio, times_pi: true }),
            };
            add(Unit::Angle(u), f)?;
        }
        let mut table = Vec::new();
        for (name, mut units) in kinds {
            units.sort_by(|a, b| a.symbol.cmp(&b.symbol));
            let canonical = units.iter().find(|u| u.factor.is_one()).map(|u| u.symbol.clone()).unwrap_or_default();
            table.push(KindInfo { name, canonical, units });
        }
        let pi_hex: String = PI_256.iter().rev().map(|limb| format!("{limb:08x}")).collect();
        Catalogue::new(table, pi_hex)
    }

    /// A catalogue from its canonical-form document (whitespace and key order are free; the
    /// arrays must already be sorted). Strict: unknown or missing fields, unsorted or
    /// duplicate entries, non-canonical numbers and every rule of [`Catalogue::new`] refuse it.
    pub fn from_json(bytes: &[u8]) -> Result<Catalogue, CatalogueError> {
        if bytes.len() > MAX_CATALOGUE_BYTES {
            return Err(CatalogueError::TooLarge);
        }
        let v = fid_canon::parse_with_limits(bytes, MAX_CATALOGUE_BYTES, CATALOGUE_DEPTH)
            .map_err(|e| CatalogueError::Json(e.code()))?;
        let root = fields(&v, &["format", "kinds", "pi_hex"], "the catalogue")?;
        if !matches!(root.get("format"), Some(CValue::Str(s)) if s == CATALOGUE_FORMAT) {
            return Err(shape(format!("format must be {CATALOGUE_FORMAT}")));
        }
        let Some(CValue::Str(pi_hex)) = root.get("pi_hex") else { return Err(shape("pi_hex must be a string")) };
        let Some(CValue::Arr(kinds)) = root.get("kinds") else { return Err(shape("kinds must be an array")) };
        if kinds.len() > MAX_KINDS {
            return Err(shape(format!("more than {MAX_KINDS} kinds")));
        }
        let mut table = Vec::with_capacity(kinds.len());
        for (i, k) in kinds.iter().enumerate() {
            let what = format!("kinds[{i}]");
            let m = fields(k, &["canonical", "kind", "units"], &what)?;
            let (Some(CValue::Str(name)), Some(CValue::Str(canonical)), Some(CValue::Arr(units))) =
                (m.get("kind"), m.get("canonical"), m.get("units"))
            else {
                return Err(shape(format!("{what}: kind and canonical must be strings, units an array")));
            };
            if units.len() > MAX_UNITS_PER_KIND {
                return Err(shape(format!("{what}: more than {MAX_UNITS_PER_KIND} units")));
            }
            let mut list = Vec::with_capacity(units.len());
            for (j, u) in units.iter().enumerate() {
                let what = format!("kinds[{i}].units[{j}]");
                let m = fields(u, &["den", "num", "pi", "symbol"], &what)?;
                let (Some(CValue::Str(symbol)), Some(CValue::Bool(pi))) = (m.get("symbol"), m.get("pi")) else {
                    return Err(shape(format!("{what}: symbol must be a string and pi a boolean")));
                };
                let num = decimal(m.get("num")).ok_or_else(|| shape(format!("{what}: num is not a decimal string")))?;
                let den = decimal(m.get("den")).ok_or_else(|| shape(format!("{what}: den is not a decimal string")))?;
                let ratio =
                    Rational::new(num, den).ok_or_else(|| shape(format!("{what}: num/den is not in lowest terms")))?;
                list.push(UnitInfo { symbol: symbol.clone(), factor: Factor { ratio, times_pi: *pi } });
            }
            table.push(KindInfo { name: name.clone(), canonical: canonical.clone(), units: list });
        }
        Catalogue::new(table, pi_hex.clone())
    }

    /// Checks a table and computes its digest. Kinds must be sorted by name and units by
    /// symbol, strictly; kind names match `^[a-z][a-z0-9_]{0,31}$`; symbols are printable
    /// ASCII without space, at most 16 bytes, and unique across the catalogue even after
    /// ASCII lowercasing; each kind has 1 to 64 units, exactly one of them with factor 1,
    /// which is `canonical`; at most 64 kinds; `pi_hex` is 64 lowercase hex digits with the
    /// top bit set (Π · 2^254 lies in [2^255, 2^256)).
    pub fn new(table: Vec<KindInfo>, pi_hex: String) -> Result<Catalogue, CatalogueError> {
        if table.len() > MAX_KINDS {
            return Err(shape(format!("more than {MAX_KINDS} kinds")));
        }
        let hex_ok = pi_hex.len() == 64 && pi_hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hex_ok || !pi_hex.bytes().next().is_some_and(|b| b >= b'8') {
            return Err(shape("pi_hex must be 64 lowercase hex digits with the top bit set"));
        }
        let mut kinds = BTreeMap::new();
        let mut factors = BTreeMap::new();
        let mut lowered = BTreeSet::new();
        let mut prev_kind: Option<String> = None;
        for (i, k) in table.into_iter().enumerate() {
            if !is_kind_name(&k.name) {
                return Err(shape(format!("kinds[{i}]: the name is not ^[a-z][a-z0-9_]{{0,31}}$")));
            }
            if prev_kind.as_ref().is_some_and(|p| *p >= k.name) {
                return Err(shape(format!("kinds[{i}]: kinds must be sorted by name without duplicates")));
            }
            if k.units.is_empty() || k.units.len() > MAX_UNITS_PER_KIND {
                return Err(shape(format!("kinds[{i}]: a kind has 1 to {MAX_UNITS_PER_KIND} units")));
            }
            let mut prev_sym: Option<&str> = None;
            let mut ones = 0usize;
            for (j, u) in k.units.iter().enumerate() {
                if !is_unit_symbol(&u.symbol) {
                    return Err(shape(format!("kinds[{i}].units[{j}]: the symbol is not 1 to 16 printable ASCII")));
                }
                if prev_sym.is_some_and(|p| p >= u.symbol.as_str()) {
                    return Err(shape(format!("kinds[{i}].units[{j}]: units must be sorted by symbol")));
                }
                if !lowered.insert(u.symbol.to_ascii_lowercase()) {
                    return Err(shape(format!("kinds[{i}].units[{j}]: the symbol is used twice (ASCII case ignored)")));
                }
                if u.factor.is_one() {
                    ones += 1;
                    if u.symbol != k.canonical {
                        return Err(shape(format!("kinds[{i}]: the unit with factor 1 must be the canonical unit")));
                    }
                }
                factors.insert(u.symbol.clone(), u.factor);
                prev_sym = Some(u.symbol.as_str());
            }
            if ones != 1 {
                return Err(shape(format!("kinds[{i}]: exactly one unit must have factor 1")));
            }
            prev_kind = Some(k.name.clone());
            kinds.insert(k.name.clone(), k);
        }
        let mut cat = Catalogue { kinds, factors, pi_hex, digest: Digest::TOMBSTONE };
        let canon = fid_canon::digest(&cat.to_value()).map_err(|e| CatalogueError::Json(e.code()))?;
        cat.digest = Digest::from(canon.0);
        Ok(cat)
    }

    /// The canonical JSON value (its RFC 8785 bytes are what [`QuantityCatalogue::digest`]
    /// hashes).
    pub fn to_value(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_string());
        let kinds = self
            .kinds
            .values()
            .map(|k| {
                let units = k
                    .units
                    .iter()
                    .map(|u| {
                        let mut o = BTreeMap::new();
                        o.insert("den".to_string(), s(&u.factor.ratio.den.to_string()));
                        o.insert("num".to_string(), s(&u.factor.ratio.num.to_string()));
                        o.insert("pi".to_string(), CValue::Bool(u.factor.times_pi));
                        o.insert("symbol".to_string(), s(&u.symbol));
                        CValue::Obj(o)
                    })
                    .collect();
                let mut o = BTreeMap::new();
                o.insert("canonical".to_string(), s(&k.canonical));
                o.insert("kind".to_string(), s(&k.name));
                o.insert("units".to_string(), CValue::Arr(units));
                CValue::Obj(o)
            })
            .collect();
        let mut o = BTreeMap::new();
        o.insert("format".to_string(), s(CATALOGUE_FORMAT));
        o.insert("kinds".to_string(), CValue::Arr(kinds));
        o.insert("pi_hex".to_string(), s(&self.pi_hex));
        CValue::Obj(o)
    }

    /// Π · 2^254 as 64 lowercase hex digits.
    pub fn pi_hex(&self) -> &str {
        &self.pi_hex
    }
}

/// The fields of an object, which must be exactly `names`.
fn fields<'v>(v: &'v CValue, names: &[&str], what: &str) -> Result<&'v BTreeMap<String, CValue>, CatalogueError> {
    let CValue::Obj(m) = v else { return Err(shape(format!("{what} must be an object"))) };
    if m.len() != names.len() || !names.iter().all(|n| m.contains_key(*n)) {
        return Err(shape(format!("{what} must have exactly the fields {}", names.join(", "))));
    }
    Ok(m)
}

/// A decimal string `[1-9][0-9]*` that fits `u64`.
fn decimal(v: Option<&CValue>) -> Option<u64> {
    let Some(CValue::Str(s)) = v else { return None };
    let canonical =
        s.bytes().next().is_some_and(|b| (b'1'..=b'9').contains(&b)) && s.bytes().all(|b| b.is_ascii_digit());
    if !canonical {
        return None;
    }
    s.parse().ok()
}

impl QuantityCatalogue for Catalogue {
    fn kind_names(&self) -> Vec<&str> {
        self.kinds.keys().map(String::as_str).collect()
    }

    fn kind(&self, name: &str) -> Option<&KindInfo> {
        self.kinds.get(name)
    }

    fn factor(&self, unit: &str) -> Option<Factor> {
        self.factors.get(unit).copied()
    }

    fn digest(&self) -> Digest {
        self.digest
    }
}

/// So `CORE.REG.VAL` can be given the facade instead of `GeoFrameUnits`.
impl QuantityKinds for Catalogue {
    fn has_kind(&self, kind: &str) -> bool {
        self.kinds.contains_key(kind)
    }

    fn accepts(&self, kind: &str, unit: &str) -> bool {
        QuantityCatalogue::accepts(self, kind, unit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rationals() {
        assert_eq!(Rational::new(254, 10000), None);
        assert_eq!(Rational::reduced(254, 10000), Rational::new(127, 5000));
        assert_eq!(Rational::new(0, 1), None);
        assert_eq!(Rational::new(1, 0), None);
        assert_eq!(Rational::reduced(0, 3), None);
    }

    #[test]
    fn names() {
        assert!(is_kind_name("length") && is_kind_name("a") && is_kind_name("dimensionless_ratio"));
        assert!(!is_kind_name("") && !is_kind_name("Length") && !is_kind_name("1a") && !is_kind_name(&"a".repeat(33)));
        assert!(is_unit_symbol("um") && is_unit_symbol("%") && !is_unit_symbol("µm") && !is_unit_symbol("a b"));
    }
}
