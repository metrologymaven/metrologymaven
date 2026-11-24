//! Wire parameters of the six balloon commands and the JSON dispatcher
//! (`RPT.BALLOON.REG`, `Design: Registry commands`). Integers travel as decimal strings;
//! unknown keys are `report.bad_params`.

use super::error::RegisterError;
use super::event::{ADD_BALLOON, LIST_BALLOON, MOVE_BALLOON, RELINK_BALLOON, REMOVE_BALLOON, RENUMBER_BALLOON};
use super::model::{changes_value, list_value, parse_u32, RenumberMode, SheetPoint};
use super::store::{AddBalloonParams, BalloonRegister, MoveBalloonParams, RegisterEnv};
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId, UuidV7Gen};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use std::collections::BTreeMap;

type Params = BTreeMap<String, CValue>;

fn object<'a>(v: &'a CValue, allowed: &[&str]) -> Result<&'a Params, RegisterError> {
    let CValue::Obj(m) = v else { return Err(RegisterError::BadParams("params must be an object")) };
    if m.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(RegisterError::BadParams("unknown parameter"));
    }
    Ok(m)
}

fn req_str<'a>(m: &'a Params, k: &'static str) -> Result<&'a str, RegisterError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(RegisterError::BadParams(k)),
    }
}

/// `None` for an absent key or JSON null.
fn opt<'a>(m: &'a Params, k: &str) -> Option<&'a CValue> {
    match m.get(k) {
        None | Some(CValue::Null) => None,
        Some(v) => Some(v),
    }
}

fn balloon_id(m: &Params) -> Result<BalloonId, RegisterError> {
    BalloonId::parse(req_str(m, "balloon")?).map_err(|_| RegisterError::BadParams("balloon"))
}

fn drawing_rev(m: &Params) -> Result<DrawingRevId, RegisterError> {
    DrawingRevId::parse(req_str(m, "drawing_rev")?).map_err(|_| RegisterError::BadParams("drawing_rev"))
}

fn sheet_id(v: &CValue) -> Result<SheetId, RegisterError> {
    match v {
        CValue::Str(s) => SheetId::parse(s).map_err(|_| RegisterError::BadParams("sheet")),
        _ => Err(RegisterError::BadParams("sheet")),
    }
}

fn characteristic(m: &Params) -> Result<CharId, RegisterError> {
    CharId::parse(req_str(m, "characteristic")?).ok_or(RegisterError::BadParams("characteristic"))
}

fn number(m: &Params) -> Result<Option<u32>, RegisterError> {
    match opt(m, "number") {
        None => Ok(None),
        v => parse_u32(v).map(Some).ok_or(RegisterError::BadParams("number")),
    }
}

fn opt_point(m: &Params, k: &str) -> Result<Option<SheetPoint>, RegisterError> {
    opt(m, k).map(SheetPoint::from_value).transpose()
}

fn point(m: &Params, k: &'static str) -> Result<SheetPoint, RegisterError> {
    opt_point(m, k)?.ok_or(RegisterError::BadParams(k))
}

impl AddBalloonParams {
    /// `{drawing_rev, sheet, characteristic, anchor, label, number?, mode?: "set"|"insert", zone?}`.
    pub fn from_value(v: &CValue) -> Result<AddBalloonParams, RegisterError> {
        let m = object(v, &["anchor", "characteristic", "drawing_rev", "label", "mode", "number", "sheet", "zone"])?;
        let insert = match opt(m, "mode") {
            None => false,
            Some(CValue::Str(s)) if s == "set" => false,
            Some(CValue::Str(s)) if s == "insert" => true,
            Some(_) => return Err(RegisterError::BadParams("mode must be set or insert")),
        };
        let zone = match opt(m, "zone") {
            None => None,
            Some(CValue::Str(s)) => Some(s.clone()),
            Some(_) => return Err(RegisterError::BadParams("zone")),
        };
        Ok(AddBalloonParams {
            drawing_rev: drawing_rev(m)?,
            sheet: sheet_id(m.get("sheet").ok_or(RegisterError::BadParams("sheet"))?)?,
            characteristic: characteristic(m)?,
            anchor: point(m, "anchor")?,
            label: point(m, "label")?,
            number: number(m)?,
            insert,
            zone,
        })
    }
}

impl MoveBalloonParams {
    /// `{balloon, anchor?, label?, sheet?}`.
    pub fn from_value(v: &CValue) -> Result<MoveBalloonParams, RegisterError> {
        let m = object(v, &["anchor", "balloon", "label", "sheet"])?;
        Ok(MoveBalloonParams {
            balloon: balloon_id(m)?,
            anchor: opt_point(m, "anchor")?,
            label: opt_point(m, "label")?,
            sheet: opt(m, "sheet").map(sheet_id).transpose()?,
        })
    }
}

/// Parameters of `report.renumber_balloon@1`: `{drawing_rev, mode, balloon?, number?}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenumberBalloonParams {
    pub drawing_rev: DrawingRevId,
    pub mode: RenumberMode,
}

impl RenumberBalloonParams {
    pub fn from_value(v: &CValue) -> Result<RenumberBalloonParams, RegisterError> {
        let m = object(v, &["balloon", "drawing_rev", "mode", "number"])?;
        let balloon = match opt(m, "balloon") {
            None => None,
            Some(_) => Some(balloon_id(m)?),
        };
        let mode = RenumberMode::from_parts(req_str(m, "mode")?, balloon, number(m)?)?;
        Ok(RenumberBalloonParams { drawing_rev: drawing_rev(m)?, mode })
    }
}

/// Runs one of the six balloon commands by its ID with JSON parameters and returns the
/// JSON result: a balloon for add, move and relink; `{"changes": [...]}` for renumber; `{}`
/// for remove; `{"balloons": [...]}` for list (R0, never journaled).
pub fn execute(
    command: &str,
    params: &CValue,
    register: &mut BalloonRegister,
    env: &RegisterEnv<'_>,
    ids: &mut dyn UuidV7Gen,
) -> Result<CValue, RegisterError> {
    match command {
        ADD_BALLOON => {
            let p = AddBalloonParams::from_value(params)?;
            Ok(register.add_balloon(env, ids, &p)?.to_value())
        }
        MOVE_BALLOON => {
            let p = MoveBalloonParams::from_value(params)?;
            Ok(register.move_balloon(env, &p)?.to_value())
        }
        RENUMBER_BALLOON => {
            let p = RenumberBalloonParams::from_value(params)?;
            Ok(changes_value(&register.renumber_balloon(p.drawing_rev, p.mode)?))
        }
        RELINK_BALLOON => {
            let m = object(params, &["balloon", "characteristic"])?;
            let (b, c) = (balloon_id(m)?, characteristic(m)?);
            Ok(register.relink_balloon(env, b, &c)?.to_value())
        }
        REMOVE_BALLOON => {
            let m = object(params, &["balloon"])?;
            register.remove_balloon(balloon_id(m)?)?;
            Ok(CValue::Obj(BTreeMap::new()))
        }
        LIST_BALLOON => {
            let m = object(params, &["drawing_rev"])?;
            Ok(list_value(&register.list(drawing_rev(m)?)))
        }
        _ => Err(RegisterError::BadParams("unknown balloon command")),
    }
}
