//! Journal events of the balloon register (`RPT.BALLOON.REG`): one event per successful
//! W2 command, carrying the handler-generated balloon ID so replay never makes a new one,
//! and the SHA-256 of the command's canonical result for the exact-replay check (`AC-7`).

use super::error::RegisterError;
use super::model::{parse_u32, parse_zone, RenumberMode, SheetPoint};
use crate::balloon::sheet::{digest_bytes, BalloonId, DrawingRevId, SheetId};
use crate::doc::text::{Text, Untrusted};
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_types::Digest;
use std::collections::BTreeMap;

/// Command and event ID: place a balloon (`W2`).
pub const ADD_BALLOON: &str = "report.add_balloon@1";
/// Command and event ID: move a balloon's anchor, label or sheet (`W2`).
pub const MOVE_BALLOON: &str = "report.move_balloon@1";
/// Command and event ID: renumber balloons in mode `set`, `insert` or `compact` (`W2`).
pub const RENUMBER_BALLOON: &str = "report.renumber_balloon@1";
/// Command and event ID: link a balloon to another characteristic (`W2`).
pub const RELINK_BALLOON: &str = "report.relink_balloon@1";
/// Command and event ID: remove a balloon, freeing its number and retiring its ID (`W2`).
pub const REMOVE_BALLOON: &str = "report.remove_balloon@1";
/// Command ID: list a drawing revision's balloons by ascending number (`R0`, not journaled).
pub const LIST_BALLOON: &str = "report.list_balloon@1";

/// Payload of [`BalloonEvent::Add`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddEvent {
    /// The UUIDv7 the handler generated; replay reads it back (`REQ-001`).
    pub id: BalloonId,
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    /// The number the caller asked for, if any.
    pub requested: Option<u32>,
    /// True for mode `insert` (numbers at or above `requested` moved up by one).
    pub insert: bool,
    /// The number assigned; replay re-derives it and compares.
    pub number: u32,
    pub zone: Option<Untrusted<Text>>,
    pub result_digest: Digest,
}

/// One journal event of the balloon register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalloonEvent {
    Add(Box<AddEvent>),
    Move {
        balloon: BalloonId,
        sheet: Option<SheetId>,
        anchor: Option<SheetPoint>,
        label: Option<SheetPoint>,
        result_digest: Digest,
    },
    Renumber {
        drawing_rev: DrawingRevId,
        mode: RenumberMode,
        result_digest: Digest,
    },
    Relink {
        balloon: BalloonId,
        characteristic: CharId,
        result_digest: Digest,
    },
    Remove {
        balloon: BalloonId,
        result_digest: Digest,
    },
}

impl BalloonEvent {
    /// The event ID (`report.<verb>_balloon@1`).
    pub fn kind(&self) -> &'static str {
        match self {
            BalloonEvent::Add(_) => ADD_BALLOON,
            BalloonEvent::Move { .. } => MOVE_BALLOON,
            BalloonEvent::Renumber { .. } => RENUMBER_BALLOON,
            BalloonEvent::Relink { .. } => RELINK_BALLOON,
            BalloonEvent::Remove { .. } => REMOVE_BALLOON,
        }
    }

    /// Canonical machine form; keys sorted, integers as decimal strings.
    pub fn to_value(&self) -> CValue {
        let mut m = BTreeMap::new();
        let mut put = |k: &str, v: CValue| {
            m.insert(k.to_string(), v);
        };
        put("kind", CValue::Str(self.kind().to_string()));
        match self {
            BalloonEvent::Add(a) => {
                put("anchor", a.anchor.to_value());
                put("characteristic", CValue::Str(a.characteristic.as_str().to_string()));
                put("drawing_rev", CValue::Str(a.drawing_rev.to_string()));
                put("id", CValue::Str(a.id.to_string()));
                put("insert", CValue::Bool(a.insert));
                put("label", a.label.to_value());
                put("number", CValue::Str(a.number.to_string()));
                put("requested", a.requested.map_or(CValue::Null, |n| CValue::Str(n.to_string())));
                put("result_digest", CValue::Str(a.result_digest.to_string()));
                put("sheet", CValue::Str(a.sheet.to_string()));
                put("zone", a.zone.as_ref().map_or(CValue::Null, Untrusted::to_value));
            }
            BalloonEvent::Move { balloon, sheet, anchor, label, result_digest } => {
                put("anchor", anchor.map_or(CValue::Null, SheetPoint::to_value));
                put("balloon", CValue::Str(balloon.to_string()));
                put("label", label.map_or(CValue::Null, SheetPoint::to_value));
                put("result_digest", CValue::Str(result_digest.to_string()));
                put("sheet", sheet.map_or(CValue::Null, |s| CValue::Str(s.to_string())));
            }
            BalloonEvent::Renumber { drawing_rev, mode, result_digest } => {
                put("drawing_rev", CValue::Str(drawing_rev.to_string()));
                put("mode", mode.to_value());
                put("result_digest", CValue::Str(result_digest.to_string()));
            }
            BalloonEvent::Relink { balloon, characteristic, result_digest } => {
                put("balloon", CValue::Str(balloon.to_string()));
                put("characteristic", CValue::Str(characteristic.as_str().to_string()));
                put("result_digest", CValue::Str(result_digest.to_string()));
            }
            BalloonEvent::Remove { balloon, result_digest } => {
                put("balloon", CValue::Str(balloon.to_string()));
                put("result_digest", CValue::Str(result_digest.to_string()));
            }
        }
        CValue::Obj(m)
    }

    /// Reads an event back. Exactly the keys of its kind; anything else is
    /// `report.store_invalid`.
    pub fn from_value(v: &CValue) -> Result<BalloonEvent, RegisterError> {
        let CValue::Obj(m) = v else { return Err(bad("event is not an object")) };
        let kind = req_str(m, "kind")?;
        let keys: &[&str] = match kind {
            ADD_BALLOON => &[
                "anchor",
                "characteristic",
                "drawing_rev",
                "id",
                "insert",
                "kind",
                "label",
                "number",
                "requested",
                "result_digest",
                "sheet",
                "zone",
            ],
            MOVE_BALLOON => &["anchor", "balloon", "kind", "label", "result_digest", "sheet"],
            RENUMBER_BALLOON => &["drawing_rev", "kind", "mode", "result_digest"],
            RELINK_BALLOON => &["balloon", "characteristic", "kind", "result_digest"],
            REMOVE_BALLOON => &["balloon", "kind", "result_digest"],
            _ => return Err(bad("unknown balloon event kind")),
        };
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
            return Err(bad("event keys"));
        }
        let result_digest = Digest::parse(req_str(m, "result_digest")?).map_err(|_| bad("result_digest"))?;
        Ok(match kind {
            ADD_BALLOON => {
                let insert = match m.get("insert") {
                    Some(CValue::Bool(b)) => *b,
                    _ => return Err(bad("insert")),
                };
                let requested = match m.get("requested") {
                    Some(CValue::Null) => None,
                    other => Some(parse_u32(other).ok_or_else(|| bad("requested"))?),
                };
                let zone = match m.get("zone") {
                    Some(CValue::Null) | None => None,
                    Some(z) => {
                        let t = Untrusted::<Text>::from_value(z).map_err(|_| bad("zone"))?;
                        Some(parse_zone(t.untrusted_value().as_str())?)
                    }
                };
                BalloonEvent::Add(Box::new(AddEvent {
                    id: BalloonId::parse(req_str(m, "id")?).map_err(|_| bad("id"))?,
                    drawing_rev: DrawingRevId::parse(req_str(m, "drawing_rev")?).map_err(|_| bad("drawing_rev"))?,
                    sheet: SheetId::parse(req_str(m, "sheet")?).map_err(|_| bad("sheet"))?,
                    characteristic: char_id(m)?,
                    anchor: point(m.get("anchor"))?,
                    label: point(m.get("label"))?,
                    requested,
                    insert,
                    number: parse_u32(m.get("number")).ok_or_else(|| bad("number"))?,
                    zone,
                    result_digest,
                }))
            }
            MOVE_BALLOON => BalloonEvent::Move {
                balloon: balloon(m)?,
                sheet: match m.get("sheet") {
                    Some(CValue::Null) => None,
                    _ => Some(SheetId::parse(req_str(m, "sheet")?).map_err(|_| bad("sheet"))?),
                },
                anchor: opt_point(m.get("anchor"))?,
                label: opt_point(m.get("label"))?,
                result_digest,
            },
            RENUMBER_BALLOON => {
                let CValue::Obj(mm) = m.get("mode").ok_or_else(|| bad("mode"))? else { return Err(bad("mode")) };
                let b = match mm.get("balloon") {
                    None => None,
                    Some(CValue::Str(s)) => Some(BalloonId::parse(s).map_err(|_| bad("mode balloon"))?),
                    Some(_) => return Err(bad("mode balloon")),
                };
                let n = match mm.get("number") {
                    None => None,
                    v => Some(parse_u32(v).ok_or_else(|| bad("mode number"))?),
                };
                let expected = 1 + usize::from(b.is_some()) + usize::from(n.is_some());
                if mm.len() != expected {
                    return Err(bad("mode keys"));
                }
                let mode = RenumberMode::from_parts(req_str(mm, "mode")?, b, n).map_err(|_| bad("mode"))?;
                BalloonEvent::Renumber {
                    drawing_rev: DrawingRevId::parse(req_str(m, "drawing_rev")?).map_err(|_| bad("drawing_rev"))?,
                    mode,
                    result_digest,
                }
            }
            RELINK_BALLOON => BalloonEvent::Relink { balloon: balloon(m)?, characteristic: char_id(m)?, result_digest },
            _ => BalloonEvent::Remove { balloon: balloon(m)?, result_digest },
        })
    }
}

/// SHA-256 of the canonical (RFC 8785) bytes of `v`.
pub(crate) fn canonical_digest(v: &CValue) -> Result<Digest, RegisterError> {
    let bytes = to_canonical_bytes(v).map_err(|e| RegisterError::Store(format!("canonicalise: {e}")))?;
    Ok(digest_bytes(&bytes))
}

fn bad(what: &str) -> RegisterError {
    RegisterError::Store(format!("balloon event: {what}"))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, RegisterError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(bad(k)),
    }
}

fn balloon(m: &BTreeMap<String, CValue>) -> Result<BalloonId, RegisterError> {
    BalloonId::parse(req_str(m, "balloon")?).map_err(|_| bad("balloon"))
}

fn char_id(m: &BTreeMap<String, CValue>) -> Result<CharId, RegisterError> {
    CharId::parse(req_str(m, "characteristic")?).ok_or_else(|| bad("characteristic"))
}

fn point(v: Option<&CValue>) -> Result<SheetPoint, RegisterError> {
    SheetPoint::from_value(v.ok_or_else(|| bad("point"))?).map_err(|_| bad("point"))
}

fn opt_point(v: Option<&CValue>) -> Result<Option<SheetPoint>, RegisterError> {
    match v {
        Some(CValue::Null) => Ok(None),
        other => point(other).map(Some),
    }
}
