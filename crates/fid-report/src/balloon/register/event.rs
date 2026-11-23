//! Canonical journal events for `RPT.BALLOON.REG` (`AC-1`, `AC-7`).

use super::error::RegisterError;
use super::model::{parse_zone, RenumberMode, SheetPoint};
use crate::balloon::sheet::{digest_bytes, BalloonId, DrawingRevId, SheetId};
use crate::doc::model::obj;
use crate::doc::text::{Text, Untrusted};
use fid_canon::{to_canonical_bytes, CValue};
use fid_gdt::chars::dprof::CharId;
use fid_types::Digest;
use std::collections::BTreeMap;

pub const ADD_BALLOON: &str = "report.add_balloon@1";
pub const MOVE_BALLOON: &str = "report.move_balloon@1";
pub const RENUMBER_BALLOON: &str = "report.renumber_balloon@1";
pub const RELINK_BALLOON: &str = "report.relink_balloon@1";
pub const REMOVE_BALLOON: &str = "report.remove_balloon@1";
pub const LIST_BALLOON: &str = "report.list_balloon@1";

/// Payload of [`BalloonEvent::Add`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddBalloonEvent {
    pub id: BalloonId,
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    pub number: u32,
    pub insert: bool,
    pub zone: Option<Untrusted<Text>>,
    pub result_digest: Digest,
}

/// Canonical journal events emitted by `RPT.BALLOON.REG` write commands (`W2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BalloonEvent {
    Add(Box<AddBalloonEvent>),
    Move {
        balloon: BalloonId,
        anchor: Option<SheetPoint>,
        label: Option<SheetPoint>,
        sheet: Option<SheetId>,
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
    },
}

impl BalloonEvent {
    pub fn kind(&self) -> &'static str {
        match self {
            BalloonEvent::Add(_) => ADD_BALLOON,
            BalloonEvent::Move { .. } => MOVE_BALLOON,
            BalloonEvent::Renumber { .. } => RENUMBER_BALLOON,
            BalloonEvent::Relink { .. } => RELINK_BALLOON,
            BalloonEvent::Remove { .. } => REMOVE_BALLOON,
        }
    }

    pub fn to_value(&self) -> CValue {
        match self {
            BalloonEvent::Add(ev) => obj([
                ("anchor", ev.anchor.to_value()),
                ("characteristic", CValue::Str(ev.characteristic.as_str().into())),
                ("drawing_rev", CValue::Str(ev.drawing_rev.to_string())),
                ("id", CValue::Str(ev.id.to_string())),
                ("insert", CValue::Bool(ev.insert)),
                ("kind", CValue::Str(ADD_BALLOON.into())),
                ("label", ev.label.to_value()),
                ("number", CValue::Str(ev.number.to_string())),
                ("result_digest", CValue::Str(ev.result_digest.to_string())),
                ("sheet", CValue::Str(ev.sheet.to_string())),
                ("zone", ev.zone.as_ref().map_or(CValue::Null, Untrusted::to_value)),
            ]),
            BalloonEvent::Move { balloon, anchor, label, sheet, result_digest } => obj([
                ("anchor", anchor.map_or(CValue::Null, SheetPoint::to_value)),
                ("balloon", CValue::Str(balloon.to_string())),
                ("kind", CValue::Str(MOVE_BALLOON.into())),
                ("label", label.map_or(CValue::Null, SheetPoint::to_value)),
                ("result_digest", CValue::Str(result_digest.to_string())),
                ("sheet", sheet.map_or(CValue::Null, |s| CValue::Str(s.to_string()))),
            ]),
            BalloonEvent::Renumber { drawing_rev, mode, result_digest } => obj([
                ("drawing_rev", CValue::Str(drawing_rev.to_string())),
                ("kind", CValue::Str(RENUMBER_BALLOON.into())),
                ("mode", mode.to_value()),
                ("result_digest", CValue::Str(result_digest.to_string())),
            ]),
            BalloonEvent::Relink { balloon, characteristic, result_digest } => obj([
                ("balloon", CValue::Str(balloon.to_string())),
                ("characteristic", CValue::Str(characteristic.as_str().into())),
                ("kind", CValue::Str(RELINK_BALLOON.into())),
                ("result_digest", CValue::Str(result_digest.to_string())),
            ]),
            BalloonEvent::Remove { balloon } => {
                obj([("balloon", CValue::Str(balloon.to_string())), ("kind", CValue::Str(REMOVE_BALLOON.into()))])
            }
        }
    }

    pub fn from_value(v: &CValue) -> Result<Self, RegisterError> {
        let CValue::Obj(m) = v else {
            return Err(RegisterError::Store("balloon event is not an object".into()));
        };
        match req_str(m, "kind")? {
            ADD_BALLOON => {
                let id = BalloonId::parse(req_str(m, "id")?).map_err(|_| RegisterError::Store("bad id".into()))?;
                let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)
                    .map_err(|_| RegisterError::Store("bad drawing_rev".into()))?;
                let sheet =
                    SheetId::parse(req_str(m, "sheet")?).map_err(|_| RegisterError::Store("bad sheet".into()))?;
                let characteristic = CharId::parse(req_str(m, "characteristic")?)
                    .ok_or_else(|| RegisterError::Store("bad characteristic".into()))?;
                let anchor = SheetPoint::from_value(
                    m.get("anchor").ok_or_else(|| RegisterError::Store("missing anchor".into()))?,
                )?;
                let label = SheetPoint::from_value(
                    m.get("label").ok_or_else(|| RegisterError::Store("missing label".into()))?,
                )?;
                let number =
                    req_str(m, "number")?.parse::<u32>().map_err(|_| RegisterError::Store("bad number".into()))?;
                let insert = match m.get("insert") {
                    Some(CValue::Bool(b)) => *b,
                    None => false,
                    _ => return Err(RegisterError::Store("bad insert".into())),
                };
                let zone = match m.get("zone") {
                    Some(CValue::Null) | None => None,
                    Some(zv) => {
                        let ut =
                            Untrusted::<Text>::from_value(zv).map_err(|_| RegisterError::Store("bad zone".into()))?;
                        Some(parse_zone(ut.untrusted_value().as_str())?)
                    }
                };
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| RegisterError::Store("bad result_digest".into()))?;
                Ok(BalloonEvent::Add(Box::new(AddBalloonEvent {
                    id,
                    drawing_rev,
                    sheet,
                    characteristic,
                    anchor,
                    label,
                    number,
                    insert,
                    zone,
                    result_digest,
                })))
            }
            MOVE_BALLOON => {
                let balloon =
                    BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::Store("bad balloon".into()))?;
                let anchor = match m.get("anchor") {
                    Some(CValue::Null) | None => None,
                    Some(v) => Some(SheetPoint::from_value(v)?),
                };
                let label = match m.get("label") {
                    Some(CValue::Null) | None => None,
                    Some(v) => Some(SheetPoint::from_value(v)?),
                };
                let sheet = match m.get("sheet") {
                    Some(CValue::Null) | None => None,
                    Some(CValue::Str(s)) => {
                        Some(SheetId::parse(s).map_err(|_| RegisterError::Store("bad sheet".into()))?)
                    }
                    _ => return Err(RegisterError::Store("bad sheet field".into())),
                };
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| RegisterError::Store("bad result_digest".into()))?;
                Ok(BalloonEvent::Move { balloon, anchor, label, sheet, result_digest })
            }
            RENUMBER_BALLOON => {
                let drawing_rev = DrawingRevId::parse(req_str(m, "drawing_rev")?)
                    .map_err(|_| RegisterError::Store("bad drawing_rev".into()))?;
                let mode = RenumberMode::from_value(
                    m.get("mode").ok_or_else(|| RegisterError::Store("missing mode".into()))?,
                )?;
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| RegisterError::Store("bad result_digest".into()))?;
                Ok(BalloonEvent::Renumber { drawing_rev, mode, result_digest })
            }
            RELINK_BALLOON => {
                let balloon =
                    BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::Store("bad balloon".into()))?;
                let characteristic = CharId::parse(req_str(m, "characteristic")?)
                    .ok_or_else(|| RegisterError::Store("bad characteristic".into()))?;
                let result_digest = Digest::parse(req_str(m, "result_digest")?)
                    .map_err(|_| RegisterError::Store("bad result_digest".into()))?;
                Ok(BalloonEvent::Relink { balloon, characteristic, result_digest })
            }
            REMOVE_BALLOON => {
                let balloon =
                    BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::Store("bad balloon".into()))?;
                Ok(BalloonEvent::Remove { balloon })
            }
            _ => Err(RegisterError::Store("unknown balloon event kind".into())),
        }
    }
}

pub(crate) fn canonical_digest(v: &CValue) -> Result<Digest, RegisterError> {
    let bytes = to_canonical_bytes(v).map_err(|e| RegisterError::Store(format!("canonicalise: {e}")))?;
    Ok(digest_bytes(&bytes))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, RegisterError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(RegisterError::Store("missing string field".into())),
    }
}
