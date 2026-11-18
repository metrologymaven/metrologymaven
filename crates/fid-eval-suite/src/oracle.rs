//! `oracle.json` (REQ-AGENT.EVAL.FORMAT-002, -003).

use crate::json::{index, join, parse_bounded, Ctx, Map};
use crate::task::wire_enum;
use crate::{ErrorCode, FormatError};
use fid_canon::CValue;
use std::collections::BTreeSet;
use std::fmt;

/// Tolerance given to an item that has a value but no tolerance, in the value's unit
/// (REQ-AGENT.EVAL.FORMAT-003, an unverified default).
pub const DEFAULT_TOL: f64 = 1e-9;

/// Closed list of accepted SI units (coherent SI; `1` is dimensionless).
///
/// Provisional and local to this crate: the future source is the L0 unit vocabulary of
/// spec node GEO.FRAME.UNITS, which this list should be replaced by once it is built.
/// `schema/eval-oracle-v1.schema.json` repeats it (a test guards the two against drift).
pub const SI_UNITS: &[&str] = &["1", "m", "m2", "m3", "rad", "sr", "s", "kg", "K", "N", "Pa"];

wire_enum! {
    /// Decision outcome of a ResultRecord (CONSTITUTION §5).
    Outcome {
        Conforms = "CONFORMS",
        Nonconforms = "NONCONFORMS",
        Indeterminate = "INDETERMINATE",
        NotDecidable = "NOT_DECIDABLE",
    }
}

wire_enum! {
    /// Evaluation basis of a ResultRecord (CONSTITUTION §5).
    Basis {
        Points = "POINTS",
        Features = "FEATURES",
        ResultsOnly = "RESULTS_ONLY",
        DirectReading = "DIRECT_READING",
    }
}

/// A finite value in an SI unit from [`SI_UNITS`]. In JSON the amount is a number or,
/// bit-exact as in CORE.JRN.CANON, `"0x"` plus 16 lowercase hex digits of its IEEE 754 bits.
#[derive(Debug, Clone, PartialEq)]
pub struct Quantity {
    pub amount: f64,
    pub unit: String,
}

/// One required ResultRecord. If `value` is set, `tol` is set too (defaulted if absent).
#[derive(Debug, Clone, PartialEq)]
pub struct OracleItem {
    pub characteristic: String,
    pub outcome: Outcome,
    pub basis: Basis,
    pub value: Option<Quantity>,
    pub tol: Option<Quantity>,
}

/// An outcome the agent must not produce for a characteristic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Forbidden {
    pub characteristic: String,
    pub outcome: Outcome,
}

/// A parsed `oracle.json`.
#[derive(Debug, Clone, PartialEq)]
pub struct Oracle {
    pub items: Vec<OracleItem>,
    pub forbidden: Vec<Forbidden>,
}

fn bits(s: &str) -> Option<f64> {
    let hex = s.strip_prefix("0x")?;
    if hex.len() != 16 || !hex.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok().map(f64::from_bits)
}

fn quantity(cx: &mut Ctx, v: Option<&CValue>, field: &str) -> Option<Quantity> {
    let q = cx.obj(v, field, &["amount", "unit"], &["amount", "unit"])?;
    let af = join(field, "amount");
    let amount = match q.get("amount") {
        None => None,
        Some(CValue::Num(f)) => Some(*f),
        Some(CValue::Str(s)) => match bits(s) {
            Some(f) if f.is_finite() => Some(f),
            Some(_) => {
                cx.err(&af, "bit pattern is not a finite number");
                None
            }
            None => {
                cx.err(&af, "expected a number or 0x plus 16 lowercase hex digits");
                None
            }
        },
        Some(_) => {
            cx.err(&af, "expected a number or 0x plus 16 lowercase hex digits");
            None
        }
    };
    let uf = join(field, "unit");
    let unit = cx.string(q.get("unit"), &uf).and_then(|u| {
        if SI_UNITS.contains(&u) {
            Some(u.to_string())
        } else {
            cx.err(&uf, format!("{u:?} is not an accepted SI unit"));
            None
        }
    });
    Some(Quantity { amount: amount?, unit: unit? })
}

fn outcome(cx: &mut Ctx, v: Option<&CValue>, field: &str) -> Option<Outcome> {
    let s = cx.string(v, field)?;
    let o = Outcome::parse(s);
    if o.is_none() {
        cx.err(field, format!("{s:?} is not one of CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE"));
    }
    o
}

fn item(cx: &mut Ctx, m: &Map, f: &str) -> Option<OracleItem> {
    let characteristic = cx.text(m.get("characteristic"), &join(f, "characteristic"));
    let outcome = outcome(cx, m.get("outcome"), &join(f, "outcome"));
    let bf = join(f, "basis");
    let basis = cx.string(m.get("basis"), &bf).and_then(|s| {
        let b = Basis::parse(s);
        if b.is_none() {
            cx.err(&bf, format!("{s:?} is not one of POINTS, FEATURES, RESULTS_ONLY, DIRECT_READING"));
        }
        b
    });
    let value = quantity(cx, m.get("value"), &join(f, "value"));
    let mut tol = quantity(cx, m.get("tol"), &join(f, "tol"));
    if m.contains_key("tol") && !m.contains_key("value") {
        cx.err(&join(f, "tol"), "a tolerance needs a value");
    }
    if let (Some(v), Some(t)) = (&value, &tol) {
        if t.unit != v.unit {
            cx.err(&join(f, "tol.unit"), format!("tolerance unit {} differs from value unit {}", t.unit, v.unit));
        }
    }
    if let Some(t) = &tol {
        if t.amount < 0.0 {
            cx.err(&join(f, "tol.amount"), "tolerance must not be negative");
        }
    }
    if tol.is_none() && !m.contains_key("tol") {
        tol = value.as_ref().map(|v| Quantity { amount: DEFAULT_TOL, unit: v.unit.clone() });
    }
    Some(OracleItem { characteristic: characteristic?, outcome: outcome?, basis: basis?, value, tol })
}

const ITEM_KEYS: &[&str] = &["characteristic", "outcome", "basis", "value", "tol"];
const ITEM_REQUIRED: &[&str] = &["characteristic", "outcome", "basis"];
const PAIR_KEYS: &[&str] = &["characteristic", "outcome"];

/// Parses an oracle file fail-closed, collecting every field error.
pub fn parse_oracle(bytes: &[u8], file: &str) -> Result<Oracle, Vec<FormatError>> {
    let root = parse_bounded(bytes, file, ErrorCode::InvalidOracle).map_err(|e| vec![e])?;
    let mut cx = Ctx::new(ErrorCode::InvalidOracle, file);
    let keys = &["items", "forbidden"];
    let Some(m) = cx.obj(Some(&root), "", keys, keys) else {
        return Err(cx.finish());
    };

    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    let raw_items = cx.arr(m.get("items"), "items").unwrap_or_default();
    for (i, v) in raw_items.iter().enumerate() {
        let f = index("items", i);
        let Some(im) = cx.obj(Some(v), &f, ITEM_KEYS, ITEM_REQUIRED) else { continue };
        if let Some(it) = item(&mut cx, im, &f) {
            if seen.insert(it.characteristic.clone()) {
                items.push(it);
            } else {
                cx.err(&join(&f, "characteristic"), "characteristic already has an item");
            }
        }
    }

    let mut forbidden: Vec<Forbidden> = Vec::new();
    let raw_forbidden = cx.arr(m.get("forbidden"), "forbidden").unwrap_or_default();
    for (j, v) in raw_forbidden.iter().enumerate() {
        let f = index("forbidden", j);
        let Some(pm) = cx.obj(Some(v), &f, PAIR_KEYS, PAIR_KEYS) else { continue };
        let characteristic = cx.text(pm.get("characteristic"), &join(&f, "characteristic"));
        let outcome = outcome(&mut cx, pm.get("outcome"), &join(&f, "outcome"));
        let (Some(characteristic), Some(outcome)) = (characteristic, outcome) else { continue };
        let pair = Forbidden { characteristic, outcome };
        if items.iter().any(|it| it.characteristic == pair.characteristic && it.outcome == pair.outcome) {
            cx.err(&f, "forbids the outcome an item requires");
        } else if forbidden.contains(&pair) {
            cx.err(&f, "repeated forbidden pair");
        } else {
            forbidden.push(pair);
        }
    }

    if !cx.has_errors() && items.is_empty() && forbidden.is_empty() {
        cx.err("items", "an oracle needs at least one item or forbidden outcome");
    }
    if cx.has_errors() {
        return Err(cx.finish());
    }
    Ok(Oracle { items, forbidden })
}

impl fmt::Display for Quantity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.amount, self.unit)
    }
}
