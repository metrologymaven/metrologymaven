//! Canonical JSON decoding and encoding (REQ-005 to REQ-007).

use crate::{
    check, echo_kind, Axis, AxisKind, ChartSeries, ChartSeriesError, ContractVersion, Digest, InvalidReason as R,
    Marker, Point, Provenance, Quantity, RefKind, RefLine, SeriesKind, Unit, UnsupportedCause, CONTRACT_MAJOR,
    CONTRACT_MINOR, MAX_DEPTH, MAX_ELEMENTS, MAX_MARKERS, MAX_POINTS, MAX_REF_LINES, MAX_SERIES_BYTES,
    MAX_VERSION_PART,
};
use fid_canon::{CValue, CanonError};
use std::collections::BTreeMap;

type Obj = BTreeMap<String, CValue>;
type Res<T> = Result<T, ChartSeriesError>;

/// A parsed series and the fields a later contract minor added that were ignored.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// The series, with its contract version as read.
    pub series: ChartSeries,
    /// Paths of the fields ignored because the series' minor is later than
    /// [`CONTRACT_MINOR`] (REQ-006), sorted by bytes. Always empty for minor 0.
    pub ignored_fields: Vec<String>,
}

fn bad<T>(field: impl Into<String>, reason: R) -> Res<T> {
    Err(ChartSeriesError::invalid(field, reason))
}

fn json_error(e: &CanonError) -> ChartSeriesError {
    let reason = match e {
        CanonError::NonFiniteNumber => R::NonFiniteNumber,
        CanonError::DuplicateKey(_) => R::DuplicateKey,
        CanonError::InputLimit => R::InputLimit,
        CanonError::InvalidUtf8 => R::InvalidUtf8,
        CanonError::Syntax(_) => R::Syntax,
    };
    ChartSeriesError::invalid("$", reason)
}

fn sub(at: &str, key: &str) -> String {
    if at == "$" {
        key.to_owned()
    } else {
        format!("{at}.{key}")
    }
}

/// Strict (minor up to [`CONTRACT_MINOR`]) or lenient (a later minor) reading.
struct Cx {
    lenient: bool,
    ignored: Vec<String>,
}

impl Cx {
    /// `v` as an object with exactly the `required` keys and any of the `optional` ones; other
    /// keys are refused (strict) or recorded (lenient).
    fn object<'a>(&mut self, v: &'a CValue, at: &str, required: &[&str], optional: &[&str]) -> Res<&'a Obj> {
        let CValue::Obj(o) = v else {
            return bad(at, R::WrongType);
        };
        for k in o.keys() {
            if !required.contains(&k.as_str()) && !optional.contains(&k.as_str()) {
                if self.lenient {
                    self.ignored.push(sub(at, k));
                } else {
                    return bad(sub(at, k), R::UnknownField);
                }
            }
        }
        if let Some(k) = required.iter().find(|k| !o.contains_key(**k)) {
            return bad(sub(at, k), R::Missing);
        }
        Ok(o)
    }
}

fn field<'a>(o: &'a Obj, at: &str, key: &str) -> Res<&'a CValue> {
    match o.get(key) {
        Some(v) => Ok(v),
        None => bad(sub(at, key), R::Missing),
    }
}

fn string<'a>(v: &'a CValue, at: &str) -> Res<&'a str> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => bad(at, R::WrongType),
    }
}

fn opt_string(o: &Obj, at: &str, key: &str) -> Res<Option<String>> {
    o.get(key).map(|v| string(v, &sub(at, key)).map(str::to_owned)).transpose()
}

/// A JSON integer (any spelling of an integral number, as JSON Schema reads it) in `0..=max`.
fn integer(v: &CValue, at: &str, max: u32) -> Res<u32> {
    let CValue::Num(f) = *v else {
        return bad(at, R::NotInteger);
    };
    if f.trunc() != f {
        return bad(at, R::NotInteger);
    }
    if !(0.0..=f64::from(max)).contains(&f) {
        return bad(at, R::OutOfRange);
    }
    // Integral and within 0..=max, so the conversion is exact.
    Ok(f as u32)
}

/// `"0x"` and 16 lowercase hex digits of the IEEE 754 bits. Non-finite values are left to
/// [`check::validate`], which names them `non_finite`.
fn f64_bits(v: &CValue, at: &str) -> Res<f64> {
    let s = string(v, at)?;
    let b = s.as_bytes();
    if b.len() != 18 || !s.starts_with("0x") || !b[2..].iter().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        return bad(at, R::BadHex);
    }
    let mut bits: u64 = 0;
    for &c in &b[2..] {
        let d = if c.is_ascii_digit() { c - b'0' } else { c - b'a' + 10 };
        bits = (bits << 4) | u64::from(d);
    }
    Ok(f64::from_bits(bits))
}

fn named<T: Copy>(v: &CValue, at: &str, lookup: fn(&str) -> Option<T>) -> Res<T> {
    match lookup(string(v, at)?) {
        Some(t) => Ok(t),
        None => bad(at, R::UnknownValue),
    }
}

fn unit(v: &CValue, at: &str) -> Res<Unit> {
    named(v, at, Unit::from_symbol)
}

fn array<'a>(v: &'a CValue, at: &str, max: usize) -> Res<&'a [CValue]> {
    match v {
        CValue::Arr(a) if a.len() > max => bad(at, R::TooMany),
        CValue::Arr(a) => Ok(a),
        _ => bad(at, R::WrongType),
    }
}

/// Parses a series from JSON text and validates it ([`check::validate`]).
///
/// The text need not be canonical; [`to_canonical_bytes`] gives the canonical form. See the
/// crate docs for the version rules (REQ-005, REQ-006).
pub fn parse(input: &[u8]) -> Res<Parsed> {
    let root =
        fid_canon::parse_with_budget(input, MAX_SERIES_BYTES, MAX_DEPTH, MAX_ELEMENTS).map_err(|e| json_error(&e))?;
    let CValue::Obj(top) = &root else {
        return bad("$", R::WrongType);
    };
    let contract_v = field(top, "$", "contract")?;
    let CValue::Obj(c) = contract_v else {
        return bad("contract", R::WrongType);
    };
    let major = integer(field(c, "contract", "major")?, "contract.major", MAX_VERSION_PART)?;
    let minor = integer(field(c, "contract", "minor")?, "contract.minor", MAX_VERSION_PART)?;
    if major != CONTRACT_MAJOR {
        let kind = match top.get("kind") {
            Some(CValue::Str(s)) => echo_kind(s),
            _ => None,
        };
        return Err(ChartSeriesError::Unsupported { major, minor, kind, cause: UnsupportedCause::Major });
    }
    let mut cx = Cx { lenient: minor > CONTRACT_MINOR, ignored: Vec::new() };
    cx.object(contract_v, "contract", &["major", "minor"], &[])?;
    cx.object(&root, "$", &["contract", "kind", "points", "provenance", "ref_lines", "x", "y"], &[])?;
    let kind = named(field(top, "$", "kind")?, "kind", SeriesKind::from_wire)?;
    let x = axis(&mut cx, field(top, "$", "x")?, "x")?;
    let y = axis(&mut cx, field(top, "$", "y")?, "y")?;
    let mut points = Vec::new();
    for (i, p) in array(field(top, "$", "points")?, "points", MAX_POINTS)?.iter().enumerate() {
        points.push(point(&mut cx, p, &format!("points[{i}]"))?);
    }
    let mut ref_lines = Vec::new();
    for (i, r) in array(field(top, "$", "ref_lines")?, "ref_lines", MAX_REF_LINES)?.iter().enumerate() {
        let at = format!("ref_lines[{i}]");
        let o = cx.object(r, &at, &["kind", "unit", "value"], &[])?;
        ref_lines.push(RefLine {
            kind: named(field(o, &at, "kind")?, &sub(&at, "kind"), RefKind::from_wire)?,
            value: f64_bits(field(o, &at, "value")?, &sub(&at, "value"))?,
            unit: unit(field(o, &at, "unit")?, &sub(&at, "unit"))?,
        });
    }
    let provenance = provenance(&mut cx, field(top, "$", "provenance")?)?;
    let series = ChartSeries { contract: ContractVersion { major, minor }, kind, x, y, points, ref_lines, provenance };
    check::validate(&series)?;
    let mut ignored_fields = cx.ignored;
    ignored_fields.sort_unstable_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(Parsed { series, ignored_fields })
}

fn axis(cx: &mut Cx, v: &CValue, at: &str) -> Res<Axis> {
    let CValue::Obj(o) = v else {
        return bad(at, R::WrongType);
    };
    let ordinal = match string(field(o, at, "kind")?, &sub(at, "kind"))? {
        "ORDINAL" => true,
        "QUANTITY" => false,
        _ => return bad(sub(at, "kind"), R::UnknownValue),
    };
    if ordinal {
        cx.object(v, at, &["kind", "label_key"], &[])?;
        let label_key = string(field(o, at, "label_key")?, &sub(at, "label_key"))?.to_owned();
        return Ok(Axis { kind: AxisKind::Ordinal, label_key });
    }
    cx.object(v, at, &["kind", "label_key", "quantity", "unit"], &[])?;
    let label_key = string(field(o, at, "label_key")?, &sub(at, "label_key"))?.to_owned();
    let quantity = named(field(o, at, "quantity")?, &sub(at, "quantity"), Quantity::from_wire)?;
    let unit = unit(field(o, at, "unit")?, &sub(at, "unit"))?;
    Ok(Axis { kind: AxisKind::Quantity { quantity, unit }, label_key })
}

fn point(cx: &mut Cx, v: &CValue, at: &str) -> Res<Point> {
    let o = cx.object(v, at, &["markers", "x", "y"], &["piece", "record", "x_label", "x_upper"])?;
    let x = f64_bits(field(o, at, "x")?, &sub(at, "x"))?;
    let y = f64_bits(field(o, at, "y")?, &sub(at, "y"))?;
    let x_upper = o.get("x_upper").map(|u| f64_bits(u, &sub(at, "x_upper"))).transpose()?;
    let x_label = opt_string(o, at, "x_label")?;
    let record = opt_string(o, at, "record")?;
    let piece = opt_string(o, at, "piece")?;
    let mut markers = Vec::new();
    let mat = sub(at, "markers");
    for (j, m) in array(field(o, at, "markers")?, &mat, MAX_MARKERS)?.iter().enumerate() {
        markers.push(marker(cx, m, &format!("{mat}[{j}]"))?);
    }
    Ok(Point { x, y, x_upper, x_label, record, piece, markers })
}

fn marker(cx: &mut Cx, v: &CValue, at: &str) -> Res<Marker> {
    let CValue::Obj(o) = v else {
        return bad(at, R::WrongType);
    };
    let kind = string(field(o, at, "kind")?, &sub(at, "kind"))?;
    match kind {
        "OUT_OF_TOLERANCE" | "INDETERMINATE" => {
            cx.object(v, at, &["kind"], &[])?;
            Ok(if kind == "INDETERMINATE" { Marker::Indeterminate } else { Marker::OutOfTolerance })
        }
        "RULE_VIOLATION" => {
            cx.object(v, at, &["kind", "rule_id", "set_id", "set_version"], &[])?;
            Ok(Marker::RuleViolation {
                set_id: string(field(o, at, "set_id")?, &sub(at, "set_id"))?.to_owned(),
                set_version: integer(field(o, at, "set_version")?, &sub(at, "set_version"), u32::MAX)?,
                rule_id: string(field(o, at, "rule_id")?, &sub(at, "rule_id"))?.to_owned(),
            })
        }
        _ => bad(sub(at, "kind"), R::UnknownValue),
    }
}

fn provenance(cx: &mut Cx, v: &CValue) -> Res<Provenance> {
    let at = "provenance";
    let o = cx.object(v, at, &["algorithm", "dataset_digest", "kernel_version"], &["binning_rule", "frozen_limits"])?;
    let d = string(field(o, at, "dataset_digest")?, "provenance.dataset_digest")?;
    let Ok(dataset_digest) = Digest::parse(d) else {
        return bad("provenance.dataset_digest", R::BadFormat);
    };
    Ok(Provenance {
        dataset_digest,
        algorithm: string(field(o, at, "algorithm")?, "provenance.algorithm")?.to_owned(),
        kernel_version: string(field(o, at, "kernel_version")?, "provenance.kernel_version")?.to_owned(),
        frozen_limits: opt_string(o, at, "frozen_limits")?,
        binning_rule: opt_string(o, at, "binning_rule")?,
    })
}

// ------------------------------------------------------------------ encoding

fn s(v: &str) -> CValue {
    CValue::Str(v.to_owned())
}

fn obj<const N: usize>(entries: [(&str, CValue); N]) -> CValue {
    CValue::Obj(entries.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

fn axis_value(a: &Axis) -> CValue {
    match a.kind {
        AxisKind::Ordinal => obj([("kind", s("ORDINAL")), ("label_key", s(&a.label_key))]),
        AxisKind::Quantity { quantity, unit } => obj([
            ("kind", s("QUANTITY")),
            ("label_key", s(&a.label_key)),
            ("quantity", s(quantity.as_str())),
            ("unit", s(unit.symbol())),
        ]),
    }
}

fn marker_value(m: &Marker) -> CValue {
    match m {
        Marker::RuleViolation { set_id, set_version, rule_id } => obj([
            ("kind", s(m.kind_str())),
            ("rule_id", s(rule_id)),
            ("set_id", s(set_id)),
            ("set_version", CValue::Num(f64::from(*set_version))),
        ]),
        _ => obj([("kind", s(m.kind_str()))]),
    }
}

fn point_value(p: &Point) -> CValue {
    let mut o = Obj::new();
    o.insert("markers".to_owned(), CValue::Arr(p.markers.iter().map(marker_value).collect()));
    o.insert("x".to_owned(), CValue::F64Bits(p.x));
    o.insert("y".to_owned(), CValue::F64Bits(p.y));
    if let Some(u) = p.x_upper {
        o.insert("x_upper".to_owned(), CValue::F64Bits(u));
    }
    for (k, v) in [("x_label", &p.x_label), ("record", &p.record), ("piece", &p.piece)] {
        if let Some(v) = v {
            o.insert(k.to_owned(), s(v));
        }
    }
    CValue::Obj(o)
}

fn series_value(series: &ChartSeries) -> CValue {
    let p = &series.provenance;
    let mut prov = Obj::new();
    prov.insert("algorithm".to_owned(), s(&p.algorithm));
    prov.insert("dataset_digest".to_owned(), s(p.dataset_digest.as_str()));
    prov.insert("kernel_version".to_owned(), s(&p.kernel_version));
    if let Some(f) = &p.frozen_limits {
        prov.insert("frozen_limits".to_owned(), s(f));
    }
    if let Some(b) = &p.binning_rule {
        prov.insert("binning_rule".to_owned(), s(b));
    }
    let refs = series
        .ref_lines
        .iter()
        .map(|r| obj([("kind", s(r.kind.as_str())), ("unit", s(r.unit.symbol())), ("value", CValue::F64Bits(r.value))]))
        .collect();
    obj([
        (
            "contract",
            obj([
                ("major", CValue::Num(f64::from(series.contract.major))),
                ("minor", CValue::Num(f64::from(series.contract.minor))),
            ]),
        ),
        ("kind", s(series.kind.as_str())),
        ("points", CValue::Arr(series.points.iter().map(point_value).collect())),
        ("provenance", CValue::Obj(prov)),
        ("ref_lines", CValue::Arr(refs)),
        ("x", axis_value(&series.x)),
        ("y", axis_value(&series.y)),
    ])
}

/// The RFC 8785 canonical bytes of a valid contract-1.0 series (REQ-007). Runs
/// [`check::validate`] first; a series of a later minor is `not_encodable` (crate docs).
pub fn to_canonical_bytes(series: &ChartSeries) -> Res<Vec<u8>> {
    check::validate(series)?;
    if series.contract.minor != CONTRACT_MINOR {
        return bad("contract.minor", R::NotEncodable);
    }
    // Validation leaves no non-finite value, the only thing the writer refuses.
    fid_canon::to_canonical_bytes(&series_value(series)).map_err(|e| json_error(&e))
}

/// The `sha256:` digest of [`to_canonical_bytes`].
pub fn digest(series: &ChartSeries) -> Res<Digest> {
    let bytes = to_canonical_bytes(series)?;
    Ok(Digest::from_bytes(fid_canon::digest_bytes(&bytes).0))
}
