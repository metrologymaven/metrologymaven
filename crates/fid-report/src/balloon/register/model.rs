//! Data model of the balloon register (`RPT.BALLOON.REG`, `Design: Data structures`).
//!
//! Integer geometry only (`SheetPoint` in `0.01 mm` from the sheet origin defined by
//! `RPT.BALLOON.SHEET`), no float type or operation (RPT float rule).

use super::error::RegisterError;
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId};
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use std::collections::{BTreeMap, BTreeSet};

/// Largest drawing zone, in bytes of UTF-8 (`REQ-010`).
pub const MAX_ZONE_BYTES: usize = 16;

/// Checks an optional drawing zone: untrusted text of at most [`MAX_ZONE_BYTES`] bytes and
/// no NUL (`REQ-010`, CONSTITUTION C6); `report.text_invalid` otherwise.
pub fn parse_zone(s: &str) -> Result<Untrusted<Text>, RegisterError> {
    if s.len() > MAX_ZONE_BYTES {
        return Err(RegisterError::TextInvalid("zone longer than 16 bytes"));
    }
    if s.contains('\0') {
        return Err(RegisterError::TextInvalid("zone holds NUL"));
    }
    Untrusted::text(s).map_err(|_| RegisterError::TextInvalid("zone"))
}

/// The characteristics a balloon may link to (consumed from `GDT.CHAR`; tests use a stub).
pub trait CharacteristicIndex {
    /// True when characteristic `c` exists.
    fn contains(&self, c: &CharId) -> bool;
}

impl CharacteristicIndex for BTreeSet<CharId> {
    fn contains(&self, c: &CharId) -> bool {
        BTreeSet::contains(self, c)
    }
}

/// A position in integer sheet coordinates, `0.01 mm` from the sheet origin (`REQ-008`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SheetPoint {
    pub x: i64,
    pub y: i64,
}

impl SheetPoint {
    pub const fn new(x: i64, y: i64) -> SheetPoint {
        SheetPoint { x, y }
    }

    /// Wire form `{"x": "<i64>", "y": "<i64>"}`.
    pub fn to_value(self) -> CValue {
        obj([("x", CValue::Str(self.x.to_string())), ("y", CValue::Str(self.y.to_string()))])
    }

    /// Reads the wire form; exactly the keys `x` and `y`, each a decimal `i64` string.
    pub fn from_value(v: &CValue) -> Result<SheetPoint, RegisterError> {
        let CValue::Obj(m) = v else { return Err(RegisterError::BadParams("point must be an object")) };
        if m.len() != 2 {
            return Err(RegisterError::BadParams("point must hold exactly x and y"));
        }
        let x = parse_i64(m.get("x")).ok_or(RegisterError::BadParams("point x"))?;
        let y = parse_i64(m.get("y")).ok_or(RegisterError::BadParams("point y"))?;
        Ok(SheetPoint { x, y })
    }

    /// True when the point lies inside `0..=width` x `0..=height` (sheet edges included).
    pub fn inside(self, width: i64, height: i64) -> bool {
        self.x >= 0 && self.y >= 0 && self.x <= width && self.y <= height
    }
}

/// One balloon on a drawing revision (`Design: Data structures`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Balloon {
    /// Stable UUIDv7 ID, never reused (`REQ-001`, `REQ-009`).
    pub id: BalloonId,
    /// Balloon number, `>= 1`, unique per drawing revision (`REQ-002`).
    pub number: u32,
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    /// The one linked characteristic (`REQ-003`).
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    /// Optional drawing zone, untrusted text of at most 16 bytes (`REQ-010`).
    pub zone: Option<Untrusted<Text>>,
}

impl Balloon {
    /// Machine form (keys sorted; integers as decimal strings; zone as untrusted text).
    pub fn to_value(&self) -> CValue {
        obj([
            ("anchor", self.anchor.to_value()),
            ("characteristic", CValue::Str(self.characteristic.as_str().to_string())),
            ("drawing_rev", CValue::Str(self.drawing_rev.to_string())),
            ("id", CValue::Str(self.id.to_string())),
            ("label", self.label.to_value()),
            ("number", CValue::Str(self.number.to_string())),
            ("sheet", CValue::Str(self.sheet.to_string())),
            ("zone", self.zone.as_ref().map_or(CValue::Null, Untrusted::to_value)),
        ])
    }
}

/// Mode of `report.renumber_balloon@1` (`REQ-004`..`REQ-006`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenumberMode {
    /// Give `balloon` the free number `number` (`REQ-004`).
    Set { balloon: BalloonId, number: u32 },
    /// Give `balloon` `number`; every other balloon at or above it moves up by one (`REQ-005`).
    Insert { balloon: BalloonId, number: u32 },
    /// Renumber all balloons of the revision to `1..=n` in their current order (`REQ-006`).
    Compact,
}

impl RenumberMode {
    /// Wire name of the mode.
    pub const fn name(self) -> &'static str {
        match self {
            RenumberMode::Set { .. } => "set",
            RenumberMode::Insert { .. } => "insert",
            RenumberMode::Compact => "compact",
        }
    }

    /// Builds a mode from its flat parameters: `set` and `insert` need `balloon` and a
    /// `number >= 1`; `compact` takes neither (`report.bad_params` otherwise).
    pub fn from_parts(mode: &str, balloon: Option<BalloonId>, number: Option<u32>) -> Result<Self, RegisterError> {
        match (mode, balloon, number) {
            ("set" | "insert", _, Some(0)) => Err(RegisterError::BadParams("number must be at least 1")),
            ("set", Some(balloon), Some(number)) => Ok(RenumberMode::Set { balloon, number }),
            ("insert", Some(balloon), Some(number)) => Ok(RenumberMode::Insert { balloon, number }),
            ("set" | "insert", _, _) => Err(RegisterError::BadParams("set and insert need balloon and number")),
            ("compact", None, None) => Ok(RenumberMode::Compact),
            ("compact", _, _) => Err(RegisterError::BadParams("compact takes no balloon or number")),
            _ => Err(RegisterError::BadParams("mode must be set, insert or compact")),
        }
    }

    /// Flat wire form: `mode`, plus `balloon` and `number` for `set` and `insert`.
    pub fn to_value(self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("mode".to_string(), CValue::Str(self.name().to_string()));
        if let RenumberMode::Set { balloon, number } | RenumberMode::Insert { balloon, number } = self {
            m.insert("balloon".to_string(), CValue::Str(balloon.to_string()));
            m.insert("number".to_string(), CValue::Str(number.to_string()));
        }
        CValue::Obj(m)
    }
}

/// One renumbering `(balloon, old, new)` reported by `report.renumber_balloon@1`.
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

/// Wire form of a renumber result: `{"changes": [...]}`, ascending by new number.
pub fn changes_value(changes: &[RenumberChange]) -> CValue {
    obj([("changes", CValue::Arr(changes.iter().map(|c| c.to_value()).collect()))])
}

/// Wire form of a list result: `{"balloons": [...]}`, ascending by number (`REQ-011`).
pub fn list_value(balloons: &[Balloon]) -> CValue {
    obj([("balloons", CValue::Arr(balloons.iter().map(Balloon::to_value).collect()))])
}

/// Parses a decimal `i64` string (no sign other than a leading `-`, no leading `+`).
pub(crate) fn parse_i64(v: Option<&CValue>) -> Option<i64> {
    match v {
        Some(CValue::Str(s)) if !s.starts_with('+') => s.parse::<i64>().ok(),
        _ => None,
    }
}

/// Parses a decimal `u32` string (no sign).
pub(crate) fn parse_u32(v: Option<&CValue>) -> Option<u32> {
    match v {
        Some(CValue::Str(s)) if !s.starts_with('+') => s.parse::<u32>().ok(),
        _ => None,
    }
}
