//! Carry balloons forward from an earlier drawing revision to a newly imported one
//! (`RPT.BALLOON.SHEET.CARRY`, `REQ-001..005`, `AC-1..AC-4`).
//!
//! Balloons are read and written exclusively through [`BalloonSink`], keeping this module
//! completely decoupled from `RPT.BALLOON.REG` so no dependency cycle arises.
//! Pure integer geometry (`0.01 mm`, top-left origin) and `BTreeMap` only.

use super::event::{canonical_digest, DrawingEvent};
use super::model::{BalloonId, DrawingRevId, SheetId, SheetInfo};
use super::store::DrawingStore;
use crate::doc::defs::ID_PATTERN;
use crate::doc::model::obj;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Registry command and canonical journal event ID (`W2`, `D0`).
pub const CARRY_DRAWING_BALLOON: &str = "report.carry_drawing_balloon@1";

const LIFECYCLE_SINCE: &str = "2026-11-22";

/// Integer sheet coordinate in `0.01 mm` (`top-left` origin, `RPT.BALLOON.SHEET`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CarryPoint {
    pub x: i64,
    pub y: i64,
}

impl CarryPoint {
    pub const fn new(x: i64, y: i64) -> Self {
        Self { x, y }
    }

    pub const fn within_bounds(self, width_cmm: i64, height_cmm: i64) -> bool {
        self.x >= 0 && self.y >= 0 && self.x <= width_cmm && self.y <= height_cmm
    }
}

/// Read-only view of one balloon on a source drawing revision returned by [`BalloonSink::list`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BalloonView {
    pub id: BalloonId,
    pub sheet: SheetId,
    /// Stable characteristic identifier (`GDT.CHAR`) linked to this balloon.
    pub characteristic: String,
    pub anchor: CarryPoint,
    pub label: CarryPoint,
    pub number: u32,
    pub zone: Option<String>,
}

/// Typed error returned by a [`BalloonSink`] implementation (`Design: Errors`, passed through).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SinkError {
    pub code: String,
    pub detail: String,
}

impl SinkError {
    pub fn new(code: impl Into<String>, detail: impl Into<String>) -> Self {
        Self { code: code.into(), detail: detail.into() }
    }
}

impl fmt::Display for SinkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for SinkError {}

/// Register-agnostic sink injected by registry wiring so `RPT.BALLOON.SHEET.CARRY` never
/// depends on `RPT.BALLOON.REG` (`Design: Trait`).
pub trait BalloonSink {
    /// Lists all balloons currently on `rev` (any order; carry sorts by `(number, id)`).
    fn list(&self, rev: DrawingRevId) -> Vec<BalloonView>;

    /// Creates one carried balloon on `rev` with the exact given `number`, characteristic link,
    /// anchor/label positions and optional zone, allocating a fresh UUIDv7 [`BalloonId`].
    #[allow(clippy::too_many_arguments)]
    fn add_with_number(
        &mut self,
        rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &str,
        anchor: CarryPoint,
        label: CarryPoint,
        number: u32,
        zone: Option<&str>,
    ) -> Result<BalloonId, SinkError>;

    /// Exact-replay variant creating the carried balloon with the journaled `forced_id`
    /// (`AC-4`). Implementations that support exact replay override this to preserve IDs.
    #[allow(clippy::too_many_arguments)]
    fn add_with_number_exact(
        &mut self,
        forced_id: BalloonId,
        rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &str,
        anchor: CarryPoint,
        label: CarryPoint,
        number: u32,
        zone: Option<&str>,
    ) -> Result<BalloonId, SinkError> {
        let _ = forced_id;
        self.add_with_number(rev, sheet, characteristic, anchor, label, number, zone)
    }

    /// Rolls back every balloon created in `ids` during an aborted carry transaction (`REQ-004`).
    fn rollback_added(&mut self, rev: DrawingRevId, ids: &[BalloonId]);
}

/// Reason a source balloon could not be carried to the target revision (`REQ-003`, `AC-2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotCarriedReason {
    /// Source balloon's sheet had no mapped target sheet in the sheet map.
    NoMappedSheet,
    /// Source balloon's anchor or label lay outside the mapped target sheet bounds.
    OffSheet,
}

impl NotCarriedReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            NotCarriedReason::NoMappedSheet => "no_mapped_sheet",
            NotCarriedReason::OffSheet => "off_sheet",
        }
    }

    pub fn parse(s: &str) -> Result<Self, CarryError> {
        match s {
            "no_mapped_sheet" => Ok(NotCarriedReason::NoMappedSheet),
            "off_sheet" => Ok(NotCarriedReason::OffSheet),
            _ => Err(CarryError::Store("invalid not_carried reason".into())),
        }
    }
}

/// Parameters of `report.carry_drawing_balloon@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarryDrawingParams {
    pub from: DrawingRevId,
    pub to: DrawingRevId,
    /// Optional explicit `(source_sheet, target_sheet)` mapping; defaults to sheet index order.
    pub sheet_map: Option<Vec<(SheetId, SheetId)>>,
    pub dry_run: bool,
}

/// Result of `report.carry_drawing_balloon@1` (`Design: Registry command`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarryDrawingResult {
    /// `(source_balloon_id, new_target_balloon_id)` pairs in ascending source balloon number order.
    pub carried: Vec<(BalloonId, BalloonId)>,
    /// `(source_balloon_id, reason)` pairs in ascending source balloon number order.
    pub not_carried: Vec<(BalloonId, NotCarriedReason)>,
    /// `true` when mutations and the journal event were committed; `false` on `--dry-run`.
    pub committed: bool,
}

impl CarryDrawingResult {
    pub fn to_value(&self) -> CValue {
        let carried = self
            .carried
            .iter()
            .map(|(old_id, new_id)| {
                obj([("from", CValue::Str(old_id.to_string())), ("to", CValue::Str(new_id.to_string()))])
            })
            .collect();
        let not_carried = self
            .not_carried
            .iter()
            .map(|(old_id, reason)| {
                obj([("balloon", CValue::Str(old_id.to_string())), ("reason", CValue::Str(reason.as_str().into()))])
            })
            .collect();
        obj([
            ("carried", CValue::Arr(carried)),
            ("committed", CValue::Bool(self.committed)),
            ("not_carried", CValue::Arr(not_carried)),
        ])
    }
}

/// Canonical journal event payload for `report.carry_drawing_balloon@1` (`REQ-001`, `AC-4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarryDrawingEvent {
    pub from: DrawingRevId,
    pub to: DrawingRevId,
    pub sheet_map: Option<Vec<(SheetId, SheetId)>>,
    pub carried: Vec<(BalloonId, BalloonId)>,
    pub not_carried: Vec<(BalloonId, NotCarriedReason)>,
    pub result_digest: Digest,
}

impl CarryDrawingEvent {
    pub fn to_value(&self) -> CValue {
        let sheet_map = match &self.sheet_map {
            None => CValue::Null,
            Some(pairs) => CValue::Arr(
                pairs
                    .iter()
                    .map(|(src, dst)| {
                        obj([("from", CValue::Str(src.to_string())), ("to", CValue::Str(dst.to_string()))])
                    })
                    .collect(),
            ),
        };
        let carried = self
            .carried
            .iter()
            .map(|(old_id, new_id)| {
                obj([("from", CValue::Str(old_id.to_string())), ("to", CValue::Str(new_id.to_string()))])
            })
            .collect();
        let not_carried = self
            .not_carried
            .iter()
            .map(|(old_id, reason)| {
                obj([("balloon", CValue::Str(old_id.to_string())), ("reason", CValue::Str(reason.as_str().into()))])
            })
            .collect();
        obj([
            ("carried", CValue::Arr(carried)),
            ("from", CValue::Str(self.from.to_string())),
            ("kind", CValue::Str(CARRY_DRAWING_BALLOON.into())),
            ("not_carried", CValue::Arr(not_carried)),
            ("result_digest", CValue::Str(self.result_digest.to_string())),
            ("sheet_map", sheet_map),
            ("to", CValue::Str(self.to.to_string())),
        ])
    }

    pub fn from_map(m: &BTreeMap<String, CValue>) -> Result<Self, CarryError> {
        let from = DrawingRevId::parse(req_str(m, "from")?).map_err(|_| CarryError::Store("bad from".into()))?;
        let to = DrawingRevId::parse(req_str(m, "to")?).map_err(|_| CarryError::Store("bad to".into()))?;
        let result_digest =
            Digest::parse(req_str(m, "result_digest")?).map_err(|_| CarryError::Store("bad result_digest".into()))?;
        let sheet_map = match m.get("sheet_map") {
            Some(CValue::Null) | None => None,
            Some(CValue::Arr(arr)) => {
                let mut pairs = Vec::with_capacity(arr.len());
                for item in arr {
                    let CValue::Obj(pm) = item else {
                        return Err(CarryError::Store("sheet_map entry must be object".into()));
                    };
                    let s = SheetId::parse(req_str(pm, "from")?)
                        .map_err(|_| CarryError::Store("bad sheet_map.from".into()))?;
                    let d =
                        SheetId::parse(req_str(pm, "to")?).map_err(|_| CarryError::Store("bad sheet_map.to".into()))?;
                    pairs.push((s, d));
                }
                Some(pairs)
            }
            _ => return Err(CarryError::Store("invalid sheet_map".into())),
        };
        let carried = match m.get("carried") {
            Some(CValue::Arr(arr)) => {
                let mut pairs = Vec::with_capacity(arr.len());
                for item in arr {
                    let CValue::Obj(pm) = item else {
                        return Err(CarryError::Store("carried entry must be object".into()));
                    };
                    let o = BalloonId::parse(req_str(pm, "from")?)
                        .map_err(|_| CarryError::Store("bad carried.from".into()))?;
                    let n =
                        BalloonId::parse(req_str(pm, "to")?).map_err(|_| CarryError::Store("bad carried.to".into()))?;
                    pairs.push((o, n));
                }
                pairs
            }
            _ => return Err(CarryError::Store("missing carried".into())),
        };
        let not_carried = match m.get("not_carried") {
            Some(CValue::Arr(arr)) => {
                let mut out = Vec::with_capacity(arr.len());
                for item in arr {
                    let CValue::Obj(pm) = item else {
                        return Err(CarryError::Store("not_carried entry must be object".into()));
                    };
                    let o = BalloonId::parse(req_str(pm, "balloon")?)
                        .map_err(|_| CarryError::Store("bad not_carried.balloon".into()))?;
                    let r = NotCarriedReason::parse(req_str(pm, "reason")?)?;
                    out.push((o, r));
                }
                out
            }
            _ => return Err(CarryError::Store("missing not_carried".into())),
        };
        Ok(CarryDrawingEvent { from, to, sheet_map, carried, not_carried, result_digest })
    }
}

/// Typed errors of `report.carry_drawing_balloon@1` (`Design: Errors`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarryError {
    /// `report.carry_target_invalid`: target already has balloons, belongs to a different part
    /// revision, equals source, or has a sheet that is not ready (`REQ-004`, `AC-3`).
    CarryTargetInvalid { reason: &'static str },
    /// `report.not_found`: source or target drawing revision does not exist.
    NotFound(&'static str),
    /// `report.bad_params`: malformed explicit sheet map.
    BadParams(&'static str),
    /// Error returned by [`BalloonSink`] passed through verbatim (`Design: Errors`).
    Sink(SinkError),
    /// Internal store / canonicalisation failure.
    Store(String),
}

impl CarryError {
    pub fn code(&self) -> &str {
        match self {
            CarryError::CarryTargetInvalid { .. } => "report.carry_target_invalid",
            CarryError::NotFound(_) => "report.not_found",
            CarryError::BadParams(_) => "report.bad_params",
            CarryError::Sink(e) => e.code.as_str(),
            CarryError::Store(_) => "report.store_invalid",
        }
    }
}

impl fmt::Display for CarryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CarryError::CarryTargetInvalid { reason } => {
                write!(f, "report.carry_target_invalid: {reason}")
            }
            CarryError::NotFound(what) => write!(f, "report.not_found: {what}"),
            CarryError::BadParams(reason) => write!(f, "report.bad_params: {reason}"),
            CarryError::Sink(e) => write!(f, "{e}"),
            CarryError::Store(detail) => write!(f, "report.store_invalid: {detail}"),
        }
    }
}

impl std::error::Error for CarryError {}

struct PlannedCarry {
    source: BalloonView,
    target_sheet: SheetId,
}

type CarryPlan = (Vec<PlannedCarry>, Vec<(BalloonId, NotCarriedReason)>);

fn validate_and_plan(
    store: &DrawingStore,
    sink: &dyn BalloonSink,
    from: DrawingRevId,
    to: DrawingRevId,
    sheet_map: Option<&[(SheetId, SheetId)]>,
) -> Result<CarryPlan, CarryError> {
    let from_rev = store.get_revision(from).ok_or(CarryError::NotFound("from_drawing_rev"))?;
    let to_rev = store.get_revision(to).ok_or(CarryError::NotFound("to_drawing_rev"))?;

    if from == to {
        return Err(CarryError::CarryTargetInvalid { reason: "same_revision" });
    }
    if from_rev.part_rev != to_rev.part_rev {
        return Err(CarryError::CarryTargetInvalid { reason: "different_part_revision" });
    }
    if !to_rev.carried_from.is_empty() || !sink.list(to).is_empty() {
        return Err(CarryError::CarryTargetInvalid { reason: "target_has_balloons" });
    }
    for &sid in &to_rev.sheets {
        if !store.ready(sid) {
            return Err(CarryError::CarryTargetInvalid { reason: "sheet_not_ready" });
        }
    }

    let resolved_map = build_sheet_map(&from_rev.sheets, &to_rev.sheets, sheet_map)?;

    let mut source_balloons = sink.list(from);
    source_balloons.sort_by_key(|b| (b.number, b.id));

    let mut planned = Vec::with_capacity(source_balloons.len());
    let mut not_carried = Vec::new();

    for b in source_balloons {
        let Some(&target_sheet) = resolved_map.get(&b.sheet) else {
            not_carried.push((b.id, NotCarriedReason::NoMappedSheet));
            continue;
        };
        let Some((w, h)) = store.bounds(target_sheet) else {
            return Err(CarryError::CarryTargetInvalid { reason: "sheet_not_ready" });
        };
        if !b.anchor.within_bounds(w, h) || !b.label.within_bounds(w, h) {
            not_carried.push((b.id, NotCarriedReason::OffSheet));
            continue;
        }
        planned.push(PlannedCarry { source: b, target_sheet });
    }

    Ok((planned, not_carried))
}

fn build_sheet_map(
    from_sheets: &[SheetId],
    to_sheets: &[SheetId],
    explicit: Option<&[(SheetId, SheetId)]>,
) -> Result<BTreeMap<SheetId, SheetId>, CarryError> {
    let mut map = BTreeMap::new();
    match explicit {
        None => {
            for (&src, &dst) in from_sheets.iter().zip(to_sheets.iter()) {
                map.insert(src, dst);
            }
        }
        Some(pairs) => {
            let from_set: BTreeSet<SheetId> = from_sheets.iter().copied().collect();
            let to_set: BTreeSet<SheetId> = to_sheets.iter().copied().collect();
            for &(src, dst) in pairs {
                if !from_set.contains(&src) {
                    return Err(CarryError::BadParams("sheet_map source sheet not in from revision"));
                }
                if !to_set.contains(&dst) {
                    return Err(CarryError::BadParams("sheet_map target sheet not in to revision"));
                }
                if map.insert(src, dst).is_some() {
                    return Err(CarryError::BadParams("duplicate source sheet in sheet_map"));
                }
            }
        }
    }
    Ok(map)
}

/// Executes `report.carry_drawing_balloon@1` (`W2`, one journal event, `--dry-run`, `REQ-001..005`).
pub fn carry_drawing_balloon(
    store: &mut DrawingStore,
    sink: &mut dyn BalloonSink,
    params: CarryDrawingParams,
) -> Result<CarryDrawingResult, CarryError> {
    let (planned, not_carried) = validate_and_plan(store, sink, params.from, params.to, params.sheet_map.as_deref())?;

    if params.dry_run {
        // Preview without mutating `sink`, `store`, or the journal.
        let preview = planned.iter().map(|p| (p.source.id, p.source.id)).collect();
        return Ok(CarryDrawingResult { carried: preview, not_carried, committed: false });
    }

    let mut carried: Vec<(BalloonId, BalloonId)> = Vec::with_capacity(planned.len());
    let mut added_ids: Vec<BalloonId> = Vec::with_capacity(planned.len());
    let mut carried_from: BTreeMap<BalloonId, BalloonId> = BTreeMap::new();

    for item in &planned {
        match sink.add_with_number(
            params.to,
            item.target_sheet,
            &item.source.characteristic,
            item.source.anchor,
            item.source.label,
            item.source.number,
            item.source.zone.as_deref(),
        ) {
            Ok(new_id) => {
                if new_id == item.source.id || carried_from.contains_key(&new_id) {
                    sink.rollback_added(params.to, &added_ids);
                    sink.rollback_added(params.to, &[new_id]);
                    return Err(CarryError::Store("sink returned duplicate or non-fresh balloon id".into()));
                }
                added_ids.push(new_id);
                carried.push((item.source.id, new_id));
                carried_from.insert(new_id, item.source.id);
            }
            Err(err) => {
                sink.rollback_added(params.to, &added_ids);
                return Err(CarryError::Sink(err));
            }
        }
    }

    let result = CarryDrawingResult { carried: carried.clone(), not_carried: not_carried.clone(), committed: true };
    let result_digest = canonical_digest(&result.to_value()).map_err(|e| CarryError::Store(e.to_string()))?;

    let event = DrawingEvent::CarryDrawingBalloon(Box::new(CarryDrawingEvent {
        from: params.from,
        to: params.to,
        sheet_map: params.sheet_map,
        carried,
        not_carried,
        result_digest,
    }));

    if let Err(e) = store.record_carried_from(params.to, carried_from, event) {
        sink.rollback_added(params.to, &added_ids);
        return Err(CarryError::Store(e.to_string()));
    }

    Ok(result)
}

/// Replays one journaled [`CarryDrawingEvent`] against `store` and `sink`, verifying exact
/// carried pairs, not-carried reasons, predecessor map and result digest (`AC-4`).
pub fn replay_carry_event(
    store: &mut DrawingStore,
    sink: &mut dyn BalloonSink,
    ev: &CarryDrawingEvent,
) -> Result<(), ReplayOutcome> {
    let (planned, not_carried) = validate_and_plan(store, sink, ev.from, ev.to, ev.sheet_map.as_deref())
        .map_err(|_| ReplayOutcome::Divergent)?;

    if not_carried != ev.not_carried || planned.len() != ev.carried.len() {
        return Err(ReplayOutcome::Divergent);
    }

    let mut carried_from = BTreeMap::new();
    let mut added_ids = Vec::with_capacity(planned.len());
    for (item, &(expected_old, expected_new)) in planned.iter().zip(ev.carried.iter()) {
        if item.source.id != expected_old {
            sink.rollback_added(ev.to, &added_ids);
            return Err(ReplayOutcome::Divergent);
        }
        let Ok(actual_new) = sink.add_with_number_exact(
            expected_new,
            ev.to,
            item.target_sheet,
            &item.source.characteristic,
            item.source.anchor,
            item.source.label,
            item.source.number,
            item.source.zone.as_deref(),
        ) else {
            sink.rollback_added(ev.to, &added_ids);
            return Err(ReplayOutcome::Divergent);
        };
        if actual_new != expected_new {
            sink.rollback_added(ev.to, &added_ids);
            sink.rollback_added(ev.to, &[actual_new]);
            return Err(ReplayOutcome::Divergent);
        }
        added_ids.push(actual_new);
        carried_from.insert(actual_new, expected_old);
    }

    let result =
        CarryDrawingResult { carried: ev.carried.clone(), not_carried: ev.not_carried.clone(), committed: true };
    let Ok(got_digest) = canonical_digest(&result.to_value()) else {
        sink.rollback_added(ev.to, &added_ids);
        return Err(ReplayOutcome::Divergent);
    };
    if got_digest != ev.result_digest {
        sink.rollback_added(ev.to, &added_ids);
        return Err(ReplayOutcome::Divergent);
    }

    store.record_carried_from(ev.to, carried_from, DrawingEvent::CarryDrawingBalloon(Box::new(ev.clone()))).map_err(
        |_| {
            sink.rollback_added(ev.to, &added_ids);
            ReplayOutcome::Divergent
        },
    )?;
    Ok(())
}

/// Returns the validated [`CommandDef`] for `report.carry_drawing_balloon@1` (`W2`, `D0`,
/// CLI `fid report carry-drawing-balloon`, MCP `report_carry_drawing_balloon`).
pub fn carry_command_def() -> Result<CommandDef, DefError> {
    let text = |s: &str| CValue::Str(s.to_string());
    let strs = |items: &[&str]| CValue::Arr(items.iter().map(|s| text(s)).collect());
    let id = |description: &str| {
        obj([("type", text("string")), ("pattern", text(ID_PATTERN)), ("description", text(description))])
    };
    let object = |required: &[&str], properties: CValue| {
        obj([
            ("type", text("object")),
            ("additionalProperties", CValue::Bool(false)),
            ("required", strs(required)),
            ("properties", properties),
        ])
    };
    let sheet_pair = object(
        &["from", "to"],
        obj([("from", id("Source sheet ID (UUIDv7).")), ("to", id("Target sheet ID (UUIDv7)."))]),
    );
    let carried_pair = object(
        &["from", "to"],
        obj([("from", id("Source balloon ID (UUIDv7).")), ("to", id("New carried balloon ID (UUIDv7)."))]),
    );
    let not_carried_entry = object(
        &["balloon", "reason"],
        obj([
            ("balloon", id("Uncarried source balloon ID (UUIDv7).")),
            ("reason", obj([("type", text("string")), ("enum", strs(&["no_mapped_sheet", "off_sheet"]))])),
        ]),
    );

    CommandDef::from_value(obj([
        ("id", text("report.carry_drawing_balloon")),
        ("major", crate::json_num::num_usize(1)),
        ("title", text("Carry balloons forward to a new drawing revision")),
        (
            "description",
            text("Copies balloons from an earlier drawing revision onto a newly imported drawing revision with fresh UUIDv7 IDs and predecessor links."),
        ),
        (
            "params_schema",
            object(
                &["from", "to"],
                obj([
                    ("dry_run", obj([("type", text("boolean")), ("default", CValue::Bool(false))])),
                    ("from", id("Source drawing revision ID (UUIDv7).")),
                    (
                        "sheet_map",
                        obj([(
                            "anyOf",
                            CValue::Arr(vec![
                                obj([("type", text("array")), ("items", sheet_pair)]),
                                obj([("type", text("null"))]),
                            ]),
                        )]),
                    ),
                    ("to", id("Target drawing revision ID (UUIDv7).")),
                ]),
            ),
        ),
        (
            "result_schema",
            object(
                &["carried", "committed", "not_carried"],
                obj([
                    ("carried", obj([("type", text("array")), ("items", carried_pair)])),
                    ("committed", obj([("type", text("boolean"))])),
                    ("not_carried", obj([("type", text("array")), ("items", not_carried_entry)])),
                ]),
            ),
        ),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("none")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[CARRY_DRAWING_BALLOON])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}

fn req_str<'a>(m: &'a BTreeMap<String, CValue>, k: &str) -> Result<&'a str, CarryError> {
    match m.get(k) {
        Some(CValue::Str(s)) => Ok(s.as_str()),
        _ => Err(CarryError::Store("missing or non-string carry event field".into())),
    }
}
