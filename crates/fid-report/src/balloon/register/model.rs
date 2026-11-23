//! Data structures for the balloon register (`RPT.BALLOON.REG`).
//!
//! Pure integer geometry (`SheetPoint` in `0.01 mm` top-left origin), `BTreeMap`-only indexes,
//! zero floating-point types or operations.

use super::error::RegisterError;
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId};
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use std::collections::BTreeMap;

/// Maximum UTF-8 byte length of a balloon's optional drawing zone (`REQ-010`).
pub const MAX_ZONE_BYTES: usize = 16;

/// Validates an optional drawing zone string (`<= 16` UTF-8 bytes, no NUL, C6 untrusted text).
pub fn parse_zone(s: &str) -> Result<Untrusted<Text>, RegisterError> {
    if s.len() > MAX_ZONE_BYTES {
        return Err(RegisterError::TextInvalid("zone exceeds 16 bytes"));
    }
    if s.contains('\0') {
        return Err(RegisterError::TextInvalid("zone contains NUL"));
    }
    Untrusted::text(s).map_err(|_| RegisterError::TextInvalid("invalid zone text"))
}

/// Point in integer `0.01 mm` sheet coordinates (origin top-left, `REQ-008`, `Design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetPoint {
    pub x: i64,
    pub y: i64,
}

impl SheetPoint {
    pub const fn new(x: i64, y: i64) -> Self {
        SheetPoint { x, y }
    }

    pub fn to_value(self) -> CValue {
        obj([("x", CValue::Str(self.x.to_string())), ("y", CValue::Str(self.y.to_string()))])
    }

    pub fn from_value(v: &CValue) -> Result<Self, RegisterError> {
        let CValue::Obj(m) = v else {
            return Err(RegisterError::BadParams("point must be an object"));
        };
        let x = req_i64(m, "x")?;
        let y = req_i64(m, "y")?;
        Ok(SheetPoint { x, y })
    }
}

/// One registered balloon linked to a single characteristic on a drawing revision (`Design`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Balloon {
    pub id: BalloonId,
    /// Positive 1-based balloon number unique within `drawing_rev`.
    pub number: u32,
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    pub zone: Option<Untrusted<Text>>,
}

impl Balloon {
    pub fn to_value(&self) -> CValue {
        obj([
            ("anchor", self.anchor.to_value()),
            ("characteristic", CValue::Str(self.characteristic.as_str().into())),
            ("drawing_rev", CValue::Str(self.drawing_rev.to_string())),
            ("id", CValue::Str(self.id.to_string())),
            ("label", self.label.to_value()),
            ("number", CValue::Str(self.number.to_string())),
            ("sheet", CValue::Str(self.sheet.to_string())),
            ("zone", self.zone.as_ref().map_or(CValue::Null, Untrusted::to_value)),
        ])
    }
}

/// Mode for `report.renumber_balloon@1` (`REQ-004..006`, `Design`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenumberMode {
    /// Give `balloon` the target `number` if free, else `report.balloon_number_taken`.
    Set { balloon: BalloonId, number: u32 },
    /// Give `balloon` the target `number` and shift every other balloon with `n >= number` up by 1.
    Insert { balloon: BalloonId, number: u32 },
    /// Renumber all balloons on `drawing_rev` to `1..=n` preserving current ascending order.
    Compact,
}

impl RenumberMode {
    pub fn to_value(self) -> CValue {
        match self {
            RenumberMode::Set { balloon, number } => obj([
                ("balloon", CValue::Str(balloon.to_string())),
                ("mode", CValue::Str("set".into())),
                ("number", CValue::Str(number.to_string())),
            ]),
            RenumberMode::Insert { balloon, number } => obj([
                ("balloon", CValue::Str(balloon.to_string())),
                ("mode", CValue::Str("insert".into())),
                ("number", CValue::Str(number.to_string())),
            ]),
            RenumberMode::Compact => obj([("mode", CValue::Str("compact".into()))]),
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, RegisterError> {
        let CValue::Obj(m) = v else {
            return Err(RegisterError::BadParams("mode must be an object"));
        };
        let mode = match m.get("mode") {
            Some(CValue::Str(s)) => s.as_str(),
            _ => return Err(RegisterError::BadParams("missing mode")),
        };
        match mode {
            "set" => {
                let balloon = BalloonId::parse(req_str(m, "balloon")?)
                    .map_err(|_| RegisterError::BadParams("invalid balloon uuid"))?;
                let number = req_u32(m, "number")?;
                Ok(RenumberMode::Set { balloon, number })
            }
            "insert" => {
                let balloon = BalloonId::parse(req_str(m, "balloon")?)
                    .map_err(|_| RegisterError::BadParams("invalid balloon uuid"))?;
                let number = req_u32(m, "number")?;
                Ok(RenumberMode::Insert { balloon, number })
            }
            "compact" => Ok(RenumberMode::Compact),
            _ => Err(RegisterError::BadParams("mode must be set, insert, or compact")),
        }
    }
}

/// One `(balloon_id, old_number, new_number)` transition reported by `report.renumber_balloon@1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenumberChange {
    pub balloon: BalloonId,
    pub old: u32,
    pub new: u32,
}

impl RenumberChange {
    pub fn to_value(self) -> CValue {
        obj([
            ("balloon", CValue::Str(self.balloon.to_string())),
            ("new", CValue::Str(self.new.to_string())),
            ("old", CValue::Str(self.old.to_string())),
        ])
    }
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, RegisterError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(RegisterError::BadParams("missing or non-string field")),
    }
}

fn req_i64(m: &BTreeMap<String, CValue>, k: &str) -> Result<i64, RegisterError> {
    req_str(m, k)?.parse::<i64>().map_err(|_| RegisterError::BadParams("invalid i64"))
}

fn req_u32(m: &BTreeMap<String, CValue>, k: &str) -> Result<u32, RegisterError> {
    req_str(m, k)?.parse::<u32>().map_err(|_| RegisterError::BadParams("invalid u32"))
}
