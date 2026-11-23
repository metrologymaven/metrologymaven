//! Per-drawing-revision balloon register, indexes, command execution and exact replay
//! (`RPT.BALLOON.REG`, `REQ-001..011`, `AC-1..AC-7`).

use super::error::RegisterError;
use super::event::{canonical_digest, AddBalloonEvent, BalloonEvent};
use super::model::{parse_zone, Balloon, RenumberChange, RenumberMode, SheetPoint};
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId, SheetInfo, UuidV7Gen};
use fid_canon::CValue;
use fid_gdt::chars::dprof::CharId;
use fid_replay::ReplayOutcome;
use std::collections::{BTreeMap, BTreeSet};

/// Parameters of `report.add_balloon@1` (`Design: Registry commands`, `AC-1..AC-4`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddBalloonParams {
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    pub number: Option<u32>,
    /// When `true` with `Some(n)`, inserts at `n` and shifts existing balloons `>= n` up by 1 (`AC-4`).
    pub insert: bool,
    pub zone: Option<String>,
}

/// Parameters of `report.move_balloon@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveBalloonParams {
    pub balloon: BalloonId,
    pub anchor: Option<SheetPoint>,
    pub label: Option<SheetPoint>,
    pub sheet: Option<SheetId>,
}

/// Parameters of `report.renumber_balloon@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenumberBalloonParams {
    pub drawing_rev: DrawingRevId,
    pub mode: RenumberMode,
}

/// Parameters of `report.relink_balloon@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelinkBalloonParams {
    pub balloon: BalloonId,
    pub characteristic: CharId,
}

#[derive(Debug, Default, Clone)]
struct RevIndex {
    by_number: BTreeMap<u32, BalloonId>,
    by_char: BTreeMap<CharId, BalloonId>,
}

/// Ordered balloon register covering all drawing revisions in a project (`Design: Data structures`).
#[derive(Debug, Default, Clone)]
pub struct BalloonRegister {
    balloons: BTreeMap<BalloonId, Balloon>,
    revs: BTreeMap<DrawingRevId, RevIndex>,
    retired_ids: BTreeSet<BalloonId>,
    journal: Vec<BalloonEvent>,
}

impl BalloonRegister {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn journal(&self) -> &[BalloonEvent] {
        &self.journal
    }

    pub fn get(&self, id: BalloonId) -> Option<&Balloon> {
        self.balloons.get(&id)
    }

    pub fn is_retired(&self, id: BalloonId) -> bool {
        self.retired_ids.contains(&id)
    }

    /// Highest balloon number currently in use on `drawing_rev` (`0` if none).
    pub fn max_number(&self, drawing_rev: DrawingRevId) -> u32 {
        self.revs.get(&drawing_rev).and_then(|idx| idx.by_number.keys().next_back().copied()).unwrap_or(0)
    }

    /// `report.list_balloon@1` (`R0`, ordered by ascending balloon number, `REQ-011`).
    pub fn list(&self, drawing_rev: DrawingRevId) -> Vec<Balloon> {
        let Some(idx) = self.revs.get(&drawing_rev) else {
            return Vec::new();
        };
        idx.by_number.values().filter_map(|id| self.balloons.get(id).cloned()).collect()
    }

    /// `report.add_balloon@1` (`W2`, `REQ-001..003`, `REQ-008`, `REQ-010`, `AC-1`, `AC-2`, `AC-4`, `AC-6`).
    pub fn add_balloon(
        &mut self,
        sheets: &dyn SheetInfo,
        ids: &mut dyn UuidV7Gen,
        p: &AddBalloonParams,
    ) -> Result<Balloon, RegisterError> {
        let zone = match &p.zone {
            Some(z) => Some(parse_zone(z)?),
            None => None,
        };
        validate_sheet_points(sheets, p.drawing_rev, p.sheet, p.anchor, p.label)?;

        // Check one-balloon-per-characteristic rule before allocating an ID (`REQ-003`).
        if let Some(rev) = self.revs.get(&p.drawing_rev) {
            if rev.by_char.contains_key(&p.characteristic) {
                return Err(RegisterError::CharAlreadyBallooned {
                    characteristic: p.characteristic.as_str().to_string(),
                });
            }
        }

        let number = match p.number {
            Some(0) => return Err(RegisterError::BadParams("balloon number must be >= 1")),
            Some(n) => {
                if !p.insert && self.revs.get(&p.drawing_rev).is_some_and(|r| r.by_number.contains_key(&n)) {
                    return Err(RegisterError::BalloonNumberTaken { number: n });
                }
                n
            }
            None => self.max_number(p.drawing_rev).checked_add(1).ok_or(RegisterError::BadParams("number overflow"))?,
        };

        let raw_uuid = ids.next_v7().map_err(|e| RegisterError::Store(e.to_string()))?;
        let id = BalloonId::new(raw_uuid).ok_or_else(|| RegisterError::Store("non-v7 uuid".into()))?;
        if self.balloons.contains_key(&id) || self.retired_ids.contains(&id) {
            return Err(RegisterError::Store("balloon ID already used or retired".into()));
        }

        let balloon = self.apply_add_with_id(
            sheets,
            id,
            p.drawing_rev,
            p.sheet,
            p.characteristic.clone(),
            p.anchor,
            p.label,
            number,
            p.insert,
            zone.clone(),
        )?;

        let result_digest = canonical_digest(&balloon.to_value())?;
        self.journal.push(BalloonEvent::Add(Box::new(AddBalloonEvent {
            id,
            drawing_rev: p.drawing_rev,
            sheet: p.sheet,
            characteristic: p.characteristic.clone(),
            anchor: p.anchor,
            label: p.label,
            number,
            insert: p.insert,
            zone,
            result_digest,
        })));
        Ok(balloon)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_add_with_id(
        &mut self,
        sheets: &dyn SheetInfo,
        id: BalloonId,
        drawing_rev: DrawingRevId,
        sheet: SheetId,
        characteristic: CharId,
        anchor: SheetPoint,
        label: SheetPoint,
        number: u32,
        insert: bool,
        zone: Option<crate::doc::text::Untrusted<crate::doc::text::Text>>,
    ) -> Result<Balloon, RegisterError> {
        validate_sheet_points(sheets, drawing_rev, sheet, anchor, label)?;
        if number == 0 {
            return Err(RegisterError::BadParams("balloon number must be >= 1"));
        }
        if self.balloons.contains_key(&id) || self.retired_ids.contains(&id) {
            return Err(RegisterError::Store("balloon ID already used or retired".into()));
        }
        let rev = self.revs.entry(drawing_rev).or_default();
        if rev.by_char.contains_key(&characteristic) {
            return Err(RegisterError::CharAlreadyBallooned { characteristic: characteristic.as_str().to_string() });
        }

        if insert {
            // Shift existing balloons with number >= target in descending order so no duplicate keys occur.
            let to_shift: Vec<(u32, BalloonId)> =
                rev.by_number.range(number..).map(|(&n, &bid)| (n, bid)).rev().collect();
            for &(n, _) in &to_shift {
                if n.checked_add(1).is_none() {
                    return Err(RegisterError::BadParams("balloon number overflow"));
                }
            }
            for (n, bid) in to_shift {
                rev.by_number.remove(&n);
                let next_n = n + 1;
                rev.by_number.insert(next_n, bid);
                if let Some(b) = self.balloons.get_mut(&bid) {
                    b.number = next_n;
                }
            }
        } else if rev.by_number.contains_key(&number) {
            return Err(RegisterError::BalloonNumberTaken { number });
        }

        let balloon =
            Balloon { id, number, drawing_rev, sheet, characteristic: characteristic.clone(), anchor, label, zone };
        rev.by_number.insert(number, id);
        rev.by_char.insert(characteristic, id);
        self.balloons.insert(id, balloon.clone());
        Ok(balloon)
    }

    /// `report.move_balloon@1` (`W2`, `REQ-008`).
    pub fn move_balloon(&mut self, sheets: &dyn SheetInfo, p: &MoveBalloonParams) -> Result<Balloon, RegisterError> {
        let updated = self.apply_move(sheets, p)?;
        let result_digest = canonical_digest(&updated.to_value())?;
        self.journal.push(BalloonEvent::Move {
            balloon: p.balloon,
            anchor: p.anchor,
            label: p.label,
            sheet: p.sheet,
            result_digest,
        });
        Ok(updated)
    }

    fn apply_move(&mut self, sheets: &dyn SheetInfo, p: &MoveBalloonParams) -> Result<Balloon, RegisterError> {
        let existing = self.balloons.get(&p.balloon).ok_or(RegisterError::NotFound("balloon"))?.clone();
        let new_sheet = p.sheet.unwrap_or(existing.sheet);
        let new_anchor = p.anchor.unwrap_or(existing.anchor);
        let new_label = p.label.unwrap_or(existing.label);
        validate_sheet_points(sheets, existing.drawing_rev, new_sheet, new_anchor, new_label)?;

        let slot = self.balloons.get_mut(&p.balloon).ok_or(RegisterError::NotFound("balloon"))?;
        slot.sheet = new_sheet;
        slot.anchor = new_anchor;
        slot.label = new_label;
        Ok(slot.clone())
    }

    /// `report.renumber_balloon@1` (`W2`, `REQ-004..007`, `AC-3`, `AC-5`).
    pub fn renumber_balloon(&mut self, p: &RenumberBalloonParams) -> Result<Vec<RenumberChange>, RegisterError> {
        let changes = self.apply_renumber(p)?;
        let val = CValue::Arr(changes.iter().copied().map(RenumberChange::to_value).collect());
        let result_digest = canonical_digest(&val)?;
        self.journal.push(BalloonEvent::Renumber { drawing_rev: p.drawing_rev, mode: p.mode, result_digest });
        Ok(changes)
    }

    fn apply_renumber(&mut self, p: &RenumberBalloonParams) -> Result<Vec<RenumberChange>, RegisterError> {
        match p.mode {
            RenumberMode::Set { balloon, number } => {
                if number == 0 {
                    return Err(RegisterError::BadParams("balloon number must be >= 1"));
                }
                let existing = self.balloons.get(&balloon).ok_or(RegisterError::NotFound("balloon"))?;
                if existing.drawing_rev != p.drawing_rev {
                    return Err(RegisterError::NotFound("balloon not on drawing revision"));
                }
                let old = existing.number;
                if old == number {
                    return Ok(Vec::new());
                }
                let rev = self.revs.get_mut(&p.drawing_rev).ok_or(RegisterError::NotFound("drawing_rev"))?;
                if rev.by_number.contains_key(&number) {
                    return Err(RegisterError::BalloonNumberTaken { number });
                }
                rev.by_number.remove(&old);
                rev.by_number.insert(number, balloon);
                if let Some(b) = self.balloons.get_mut(&balloon) {
                    b.number = number;
                }
                Ok(vec![RenumberChange { balloon, old, new: number }])
            }
            RenumberMode::Insert { balloon, number } => {
                if number == 0 {
                    return Err(RegisterError::BadParams("balloon number must be >= 1"));
                }
                let existing = self.balloons.get(&balloon).ok_or(RegisterError::NotFound("balloon"))?;
                if existing.drawing_rev != p.drawing_rev {
                    return Err(RegisterError::NotFound("balloon not on drawing revision"));
                }
                let old = existing.number;
                let rev = self.revs.get_mut(&p.drawing_rev).ok_or(RegisterError::NotFound("drawing_rev"))?;

                // Remove target balloon first, then shift every remaining balloon with n >= number up by 1 in descending order.
                rev.by_number.remove(&old);
                let to_shift: Vec<(u32, BalloonId)> =
                    rev.by_number.range(number..).map(|(&n, &bid)| (n, bid)).rev().collect();
                for &(n, _) in &to_shift {
                    if n.checked_add(1).is_none() {
                        rev.by_number.insert(old, balloon);
                        return Err(RegisterError::BadParams("balloon number overflow"));
                    }
                }
                let mut changes = Vec::with_capacity(to_shift.len() + 1);
                for (n, bid) in to_shift {
                    rev.by_number.remove(&n);
                    let next_n = n + 1;
                    rev.by_number.insert(next_n, bid);
                    if let Some(b) = self.balloons.get_mut(&bid) {
                        b.number = next_n;
                    }
                    changes.push(RenumberChange { balloon: bid, old: n, new: next_n });
                }
                rev.by_number.insert(number, balloon);
                if let Some(b) = self.balloons.get_mut(&balloon) {
                    b.number = number;
                }
                if old != number {
                    changes.push(RenumberChange { balloon, old, new: number });
                }
                changes.sort_by_key(|c| c.new);
                Ok(changes)
            }
            RenumberMode::Compact => {
                let Some(rev) = self.revs.get_mut(&p.drawing_rev) else {
                    return Ok(Vec::new());
                };
                let ordered: Vec<(u32, BalloonId)> = rev.by_number.iter().map(|(&n, &bid)| (n, bid)).collect();
                rev.by_number.clear();
                let mut changes = Vec::new();
                for (idx, (old, bid)) in ordered.into_iter().enumerate() {
                    let new = (idx as u32) + 1;
                    rev.by_number.insert(new, bid);
                    if let Some(b) = self.balloons.get_mut(&bid) {
                        b.number = new;
                    }
                    if old != new {
                        changes.push(RenumberChange { balloon: bid, old, new });
                    }
                }
                Ok(changes)
            }
        }
    }

    /// `report.relink_balloon@1` (`W2`, `REQ-003`).
    pub fn relink_balloon(&mut self, p: &RelinkBalloonParams) -> Result<Balloon, RegisterError> {
        let updated = self.apply_relink(p)?;
        let result_digest = canonical_digest(&updated.to_value())?;
        self.journal.push(BalloonEvent::Relink {
            balloon: p.balloon,
            characteristic: p.characteristic.clone(),
            result_digest,
        });
        Ok(updated)
    }

    fn apply_relink(&mut self, p: &RelinkBalloonParams) -> Result<Balloon, RegisterError> {
        let existing = self.balloons.get(&p.balloon).ok_or(RegisterError::NotFound("balloon"))?.clone();
        if existing.characteristic == p.characteristic {
            return Ok(existing);
        }
        let rev = self.revs.get_mut(&existing.drawing_rev).ok_or(RegisterError::NotFound("drawing_rev"))?;
        if rev.by_char.contains_key(&p.characteristic) {
            return Err(RegisterError::CharAlreadyBallooned { characteristic: p.characteristic.as_str().to_string() });
        }
        rev.by_char.remove(&existing.characteristic);
        rev.by_char.insert(p.characteristic.clone(), p.balloon);
        let slot = self.balloons.get_mut(&p.balloon).ok_or(RegisterError::NotFound("balloon"))?;
        slot.characteristic = p.characteristic.clone();
        Ok(slot.clone())
    }

    /// `report.remove_balloon@1` (`W2`, `REQ-009`: frees number, permanently retires ID).
    pub fn remove_balloon(&mut self, balloon: BalloonId) -> Result<(), RegisterError> {
        self.apply_remove(balloon)?;
        self.journal.push(BalloonEvent::Remove { balloon });
        Ok(())
    }

    fn apply_remove(&mut self, balloon: BalloonId) -> Result<(), RegisterError> {
        let existing = self.balloons.remove(&balloon).ok_or(RegisterError::NotFound("balloon"))?;
        if let Some(rev) = self.revs.get_mut(&existing.drawing_rev) {
            rev.by_number.remove(&existing.number);
            rev.by_char.remove(&existing.characteristic);
        }
        self.retired_ids.insert(balloon);
        Ok(())
    }
}

fn validate_sheet_points(
    sheets: &dyn SheetInfo,
    drawing_rev: DrawingRevId,
    sheet: SheetId,
    anchor: SheetPoint,
    label: SheetPoint,
) -> Result<(), RegisterError> {
    let owner = sheets.revision(sheet).ok_or(RegisterError::NotFound("sheet"))?;
    if owner != drawing_rev {
        return Err(RegisterError::BadParams("sheet does not belong to drawing revision"));
    }
    let (w, h) = sheets.bounds(sheet).ok_or(RegisterError::NotFound("sheet bounds not ready"))?;
    for pt in [anchor, label] {
        if pt.x < 0 || pt.y < 0 || pt.x > w || pt.y > h {
            return Err(RegisterError::BalloonOffSheet { x: pt.x, y: pt.y, width_cmm: w, height_cmm: h });
        }
    }
    Ok(())
}

/// Result of exact replay over `BalloonEvent`s (`AC-7`).
#[derive(Debug, Clone)]
pub struct BalloonReplayRun {
    pub outcome: ReplayOutcome,
    pub first_mismatch: Option<usize>,
    pub register: BalloonRegister,
}

/// Replays `events` in exact order against `sheets` without generating any fresh IDs (`AC-7`).
pub fn replay_exact(events: &[BalloonEvent], sheets: &dyn SheetInfo) -> BalloonReplayRun {
    let mut reg = BalloonRegister::new();
    for (idx, ev) in events.iter().enumerate() {
        let ok = match ev {
            BalloonEvent::Add(add) => reg
                .apply_add_with_id(
                    sheets,
                    add.id,
                    add.drawing_rev,
                    add.sheet,
                    add.characteristic.clone(),
                    add.anchor,
                    add.label,
                    add.number,
                    add.insert,
                    add.zone.clone(),
                )
                .and_then(|b| canonical_digest(&b.to_value()).map(|d| d == add.result_digest)),
            BalloonEvent::Move { balloon, anchor, label, sheet, result_digest } => reg
                .apply_move(
                    sheets,
                    &MoveBalloonParams { balloon: *balloon, anchor: *anchor, label: *label, sheet: *sheet },
                )
                .and_then(|b| canonical_digest(&b.to_value()).map(|d| d == *result_digest)),
            BalloonEvent::Renumber { drawing_rev, mode, result_digest } => {
                reg.apply_renumber(&RenumberBalloonParams { drawing_rev: *drawing_rev, mode: *mode }).and_then(|ch| {
                    let v = CValue::Arr(ch.into_iter().map(RenumberChange::to_value).collect());
                    canonical_digest(&v).map(|d| d == *result_digest)
                })
            }
            BalloonEvent::Relink { balloon, characteristic, result_digest } => reg
                .apply_relink(&RelinkBalloonParams { balloon: *balloon, characteristic: characteristic.clone() })
                .and_then(|b| canonical_digest(&b.to_value()).map(|d| d == *result_digest)),
            BalloonEvent::Remove { balloon } => reg.apply_remove(*balloon).map(|()| true),
        };
        match ok {
            Ok(true) => reg.journal.push(ev.clone()),
            _ => {
                return BalloonReplayRun {
                    outcome: ReplayOutcome::Divergent,
                    first_mismatch: Some(idx),
                    register: reg,
                };
            }
        }
    }
    BalloonReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, register: reg }
}

/// Registry-wiring adapter implementing [`crate::balloon::sheet::BalloonSink`] over
/// [`BalloonRegister`] (`RPT.BALLOON.SHEET.CARRY`).
///
/// Mutations are applied directly to the register's indexed state without appending per-balloon
/// `add_balloon` journal events, because `report.carry_drawing_balloon@1` commits one single
/// journal event for the whole carry (`REQ-RPT.BALLOON.SHEET.CARRY-001`).
pub struct RegisterBalloonSink<'a> {
    register: &'a mut BalloonRegister,
    ids: &'a mut dyn UuidV7Gen,
}

impl<'a> RegisterBalloonSink<'a> {
    pub fn new(register: &'a mut BalloonRegister, ids: &'a mut dyn UuidV7Gen) -> Self {
        Self { register, ids }
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_carried(
        &mut self,
        id: BalloonId,
        rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &str,
        anchor: crate::balloon::sheet::CarryPoint,
        label: crate::balloon::sheet::CarryPoint,
        number: u32,
        zone: Option<&str>,
    ) -> Result<BalloonId, crate::balloon::sheet::SinkError> {
        use crate::balloon::sheet::SinkError;
        if number == 0 {
            return Err(SinkError::new("report.bad_params", "balloon number must be >= 1"));
        }
        if self.register.balloons.contains_key(&id) || self.register.retired_ids.contains(&id) {
            return Err(SinkError::new("report.store_invalid", "balloon id already exists or retired"));
        }
        let char_id = CharId::parse(characteristic)
            .ok_or_else(|| SinkError::new("report.bad_params", "invalid characteristic id"))?;
        let parsed_zone = match zone {
            Some(z) => Some(parse_zone(z).map_err(|e| SinkError::new(e.code(), e.to_string()))?),
            None => None,
        };
        if let Some(idx) = self.register.revs.get(&rev) {
            if idx.by_number.contains_key(&number) {
                return Err(SinkError::new(
                    "report.balloon_number_taken",
                    format!("balloon number {number} already taken on target revision"),
                ));
            }
            if idx.by_char.contains_key(&char_id) {
                return Err(SinkError::new(
                    "report.char_already_ballooned",
                    format!("characteristic {} already ballooned on target revision", char_id.as_str()),
                ));
            }
        }
        let b = Balloon {
            id,
            drawing_rev: rev,
            sheet,
            characteristic: char_id.clone(),
            number,
            anchor: SheetPoint::new(anchor.x, anchor.y),
            label: SheetPoint::new(label.x, label.y),
            zone: parsed_zone,
        };
        let idx = self.register.revs.entry(rev).or_default();
        idx.by_number.insert(number, id);
        idx.by_char.insert(char_id, id);
        self.register.balloons.insert(id, b);
        Ok(id)
    }
}

impl crate::balloon::sheet::BalloonSink for RegisterBalloonSink<'_> {
    fn list(&self, rev: DrawingRevId) -> Vec<crate::balloon::sheet::BalloonView> {
        use crate::balloon::sheet::{BalloonView, CarryPoint};
        self.register
            .list(rev)
            .into_iter()
            .map(|b| BalloonView {
                id: b.id,
                sheet: b.sheet,
                characteristic: b.characteristic.as_str().to_string(),
                anchor: CarryPoint::new(b.anchor.x, b.anchor.y),
                label: CarryPoint::new(b.label.x, b.label.y),
                number: b.number,
                zone: b.zone.map(|z| z.untrusted_value().as_str().to_string()),
            })
            .collect()
    }

    fn add_with_number(
        &mut self,
        rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &str,
        anchor: crate::balloon::sheet::CarryPoint,
        label: crate::balloon::sheet::CarryPoint,
        number: u32,
        zone: Option<&str>,
    ) -> Result<BalloonId, crate::balloon::sheet::SinkError> {
        use crate::balloon::sheet::SinkError;
        let uuid = self.ids.next_v7().map_err(|e| SinkError::new(e.code(), e.to_string()))?;
        let id = BalloonId::new(uuid)
            .ok_or_else(|| SinkError::new("report.store_invalid", "id generator did not return UUIDv7"))?;
        self.insert_carried(id, rev, sheet, characteristic, anchor, label, number, zone)
    }

    fn add_with_number_exact(
        &mut self,
        forced_id: BalloonId,
        rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &str,
        anchor: crate::balloon::sheet::CarryPoint,
        label: crate::balloon::sheet::CarryPoint,
        number: u32,
        zone: Option<&str>,
    ) -> Result<BalloonId, crate::balloon::sheet::SinkError> {
        self.insert_carried(forced_id, rev, sheet, characteristic, anchor, label, number, zone)
    }

    fn rollback_added(&mut self, rev: DrawingRevId, ids: &[BalloonId]) {
        for &id in ids {
            if let Some(removed) = self.register.balloons.remove(&id) {
                if let Some(idx) = self.register.revs.get_mut(&rev) {
                    idx.by_number.remove(&removed.number);
                    idx.by_char.remove(&removed.characteristic);
                }
            }
        }
        if let Some(idx) = self.register.revs.get(&rev) {
            if idx.by_number.is_empty() && idx.by_char.is_empty() {
                self.register.revs.remove(&rev);
            }
        }
    }
}
