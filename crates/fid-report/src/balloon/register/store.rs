//! The balloon register: per-drawing-revision number and link indexes, retired IDs and the
//! register's journal (`RPT.BALLOON.REG`, `Design: Data structures`, `Design: Algorithm`).
//!
//! Every command first checks everything and builds its full outcome, and only then changes
//! state and journals one event, so a failed command changes nothing and journals nothing.
//! Replay ([`super::replay`]) re-runs the same `apply_*` steps with the IDs read back from
//! the events.

use super::error::RegisterError;
use super::event::{canonical_digest, AddEvent, BalloonEvent};
use super::model::{changes_value, parse_zone, Balloon, CharacteristicIndex, RenumberChange, RenumberMode, SheetPoint};
use crate::balloon::sheet::{BalloonId, DrawingRevId, SheetId, SheetInfo, UuidV7Gen};
use fid_gdt::chars::dprof::CharId;
use std::collections::{BTreeMap, BTreeSet};

/// Parameters of `report.add_balloon@1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddBalloonParams {
    pub drawing_rev: DrawingRevId,
    pub sheet: SheetId,
    pub characteristic: CharId,
    pub anchor: SheetPoint,
    pub label: SheetPoint,
    /// Explicit number; `None` takes the highest number in use + 1 (`REQ-001`).
    pub number: Option<u32>,
    /// Mode `insert`: take `number` and move every balloon at or above it up by one
    /// (`AC-4`); needs `number`.
    pub insert: bool,
    pub zone: Option<String>,
}

/// Parameters of `report.move_balloon@1`; at least one of the three fields is given.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MoveBalloonParams {
    pub balloon: BalloonId,
    pub anchor: Option<SheetPoint>,
    pub label: Option<SheetPoint>,
    pub sheet: Option<SheetId>,
}

/// Per-drawing-revision indexes (`Design: Data structures`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct RevIndex {
    by_number: BTreeMap<u32, BalloonId>,
    by_char: BTreeMap<CharId, BalloonId>,
}

/// The balloon register of a project, all drawing revisions.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BalloonRegister {
    balloons: BTreeMap<BalloonId, Balloon>,
    revs: BTreeMap<DrawingRevId, RevIndex>,
    retired_ids: BTreeSet<BalloonId>,
    journal: Vec<BalloonEvent>,
}

/// The read-only context every write command checks against.
pub struct RegisterEnv<'a> {
    /// Sheet bounds and owning revision (`RPT.BALLOON.SHEET`).
    pub sheets: &'a dyn SheetInfo,
    /// The characteristics a balloon may link to (`GDT.CHAR`).
    pub chars: &'a dyn CharacteristicIndex,
}

impl BalloonRegister {
    pub fn new() -> BalloonRegister {
        BalloonRegister::default()
    }

    /// The register's journal: one event per successful W2 command, in order.
    pub fn journal(&self) -> &[BalloonEvent] {
        &self.journal
    }

    /// A live balloon by ID.
    pub fn get(&self, id: BalloonId) -> Option<&Balloon> {
        self.balloons.get(&id)
    }

    /// True when `id` belonged to a removed balloon (never reused, `REQ-009`).
    pub fn is_retired(&self, id: BalloonId) -> bool {
        self.retired_ids.contains(&id)
    }

    /// Highest balloon number in use on `drawing_rev`, 0 when none.
    pub fn max_number(&self, drawing_rev: DrawingRevId) -> u32 {
        self.revs.get(&drawing_rev).and_then(|r| r.by_number.keys().next_back().copied()).unwrap_or(0)
    }

    /// `report.list_balloon@1` (R0): the revision's balloons by ascending number (`REQ-011`).
    pub fn list(&self, drawing_rev: DrawingRevId) -> Vec<Balloon> {
        let Some(rev) = self.revs.get(&drawing_rev) else { return Vec::new() };
        rev.by_number.values().filter_map(|id| self.balloons.get(id).cloned()).collect()
    }

    // ------------------------------------------------------------------ commands (W2)

    /// `report.add_balloon@1`: places a balloon with a new UUIDv7 ID and journals one event
    /// (`REQ-001`..`REQ-003`, `REQ-008`, `REQ-010`).
    pub fn add_balloon(
        &mut self,
        env: &RegisterEnv<'_>,
        ids: &mut dyn UuidV7Gen,
        p: &AddBalloonParams,
    ) -> Result<Balloon, RegisterError> {
        let zone = match &p.zone {
            Some(z) => Some(parse_zone(z)?),
            None => None,
        };
        let number =
            self.check_add(env, p.drawing_rev, p.sheet, &p.characteristic, p.anchor, p.label, p.number, p.insert)?;
        let raw = ids.next_v7().map_err(|e| RegisterError::Store(format!("id generator: {e}")))?;
        let id =
            BalloonId::new(raw).ok_or_else(|| RegisterError::Store("id generator did not return a UUIDv7".into()))?;
        if self.balloons.contains_key(&id) || self.retired_ids.contains(&id) {
            return Err(RegisterError::Store("id generator returned a used balloon ID".into()));
        }
        let balloon = Balloon {
            id,
            number,
            drawing_rev: p.drawing_rev,
            sheet: p.sheet,
            characteristic: p.characteristic.clone(),
            anchor: p.anchor,
            label: p.label,
            zone,
        };
        let result_digest = canonical_digest(&balloon.to_value())?;
        let event = BalloonEvent::Add(Box::new(AddEvent {
            id,
            drawing_rev: p.drawing_rev,
            sheet: p.sheet,
            characteristic: p.characteristic.clone(),
            anchor: p.anchor,
            label: p.label,
            requested: p.number,
            insert: p.insert,
            number,
            zone: balloon.zone.clone(),
            result_digest,
        }));
        self.commit_add(balloon.clone(), p.insert);
        self.journal.push(event);
        Ok(balloon)
    }

    /// `report.move_balloon@1`: new anchor, label or sheet (same drawing revision), checked
    /// against the sheet bounds (`REQ-008`).
    pub fn move_balloon(&mut self, env: &RegisterEnv<'_>, p: &MoveBalloonParams) -> Result<Balloon, RegisterError> {
        let moved = self.check_move(env, p)?;
        let result_digest = canonical_digest(&moved.to_value())?;
        self.commit_move(moved.clone());
        self.journal.push(BalloonEvent::Move {
            balloon: p.balloon,
            sheet: p.sheet,
            anchor: p.anchor,
            label: p.label,
            result_digest,
        });
        Ok(moved)
    }

    /// `report.renumber_balloon@1` in mode `set`, `insert` or `compact`; IDs and links never
    /// change (`REQ-004`..`REQ-007`). Returns the changed numbers, ascending by new number.
    pub fn renumber_balloon(
        &mut self,
        drawing_rev: DrawingRevId,
        mode: RenumberMode,
    ) -> Result<Vec<RenumberChange>, RegisterError> {
        let changes = self.plan_renumber(drawing_rev, mode)?;
        let result_digest = canonical_digest(&changes_value(&changes))?;
        self.commit_renumber(drawing_rev, &changes);
        self.journal.push(BalloonEvent::Renumber { drawing_rev, mode, result_digest });
        Ok(changes)
    }

    /// `report.relink_balloon@1`: links the balloon to another characteristic
    /// (`REQ-003`); relinking to its own characteristic changes nothing.
    pub fn relink_balloon(
        &mut self,
        env: &RegisterEnv<'_>,
        balloon: BalloonId,
        characteristic: &CharId,
    ) -> Result<Balloon, RegisterError> {
        let relinked = self.check_relink(env, balloon, characteristic)?;
        let result_digest = canonical_digest(&relinked.to_value())?;
        self.commit_relink(relinked.clone());
        self.journal.push(BalloonEvent::Relink { balloon, characteristic: characteristic.clone(), result_digest });
        Ok(relinked)
    }

    /// `report.remove_balloon@1`: frees the number and retires the ID for ever (`REQ-009`).
    pub fn remove_balloon(&mut self, balloon: BalloonId) -> Result<(), RegisterError> {
        let removed = self.balloons.get(&balloon).ok_or(RegisterError::NotFound("balloon"))?;
        let result_digest = canonical_digest(&removed.to_value())?;
        self.commit_remove(balloon);
        self.journal.push(BalloonEvent::Remove { balloon, result_digest });
        Ok(())
    }

    // ------------------------------------------------------------------ checks (no change)

    /// Checks an add and returns the number to assign. Order: number params, sheet,
    /// positions, characteristic, number in use (`REQ-002`), link (`REQ-003`).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn check_add(
        &self,
        env: &RegisterEnv<'_>,
        drawing_rev: DrawingRevId,
        sheet: SheetId,
        characteristic: &CharId,
        anchor: SheetPoint,
        label: SheetPoint,
        number: Option<u32>,
        insert: bool,
    ) -> Result<u32, RegisterError> {
        if number == Some(0) {
            return Err(RegisterError::BadParams("number must be at least 1"));
        }
        if insert && number.is_none() {
            return Err(RegisterError::BadParams("mode insert needs a number"));
        }
        check_points(env.sheets, drawing_rev, sheet, &[anchor, label])?;
        if !env.chars.contains(characteristic) {
            return Err(RegisterError::NotFound("characteristic"));
        }
        let rev = self.revs.get(&drawing_rev);
        if let (false, Some(n), Some(r)) = (insert, number, rev) {
            if r.by_number.contains_key(&n) {
                return Err(RegisterError::BalloonNumberTaken { number: n });
            }
        }
        if rev.is_some_and(|r| r.by_char.contains_key(characteristic)) {
            return Err(RegisterError::CharAlreadyBallooned { characteristic: characteristic.as_str().to_string() });
        }
        match number {
            Some(n) => {
                // Insert moves every number at or above `n` up by one; u32::MAX cannot move.
                if insert && rev.is_some_and(|r| r.by_number.contains_key(&u32::MAX)) {
                    return Err(RegisterError::BadParams("balloon number overflow"));
                }
                Ok(n)
            }
            None => {
                self.max_number(drawing_rev).checked_add(1).ok_or(RegisterError::BadParams("balloon number overflow"))
            }
        }
    }

    pub(super) fn check_move(&self, env: &RegisterEnv<'_>, p: &MoveBalloonParams) -> Result<Balloon, RegisterError> {
        if p.anchor.is_none() && p.label.is_none() && p.sheet.is_none() {
            return Err(RegisterError::BadParams("move needs anchor, label or sheet"));
        }
        let mut b = self.balloons.get(&p.balloon).ok_or(RegisterError::NotFound("balloon"))?.clone();
        b.sheet = p.sheet.unwrap_or(b.sheet);
        b.anchor = p.anchor.unwrap_or(b.anchor);
        b.label = p.label.unwrap_or(b.label);
        check_points(env.sheets, b.drawing_rev, b.sheet, &[b.anchor, b.label])?;
        Ok(b)
    }

    pub(super) fn check_relink(
        &self,
        env: &RegisterEnv<'_>,
        balloon: BalloonId,
        characteristic: &CharId,
    ) -> Result<Balloon, RegisterError> {
        let mut b = self.balloons.get(&balloon).ok_or(RegisterError::NotFound("balloon"))?.clone();
        if !env.chars.contains(characteristic) {
            return Err(RegisterError::NotFound("characteristic"));
        }
        let holder = self.revs.get(&b.drawing_rev).and_then(|r| r.by_char.get(characteristic));
        if holder.is_some_and(|h| *h != balloon) {
            return Err(RegisterError::CharAlreadyBallooned { characteristic: characteristic.as_str().to_string() });
        }
        b.characteristic = characteristic.clone();
        Ok(b)
    }

    /// Plans a renumbering without changing anything: the full list of `(balloon, old,
    /// new)` for balloons whose number changes, ascending by new number.
    pub(super) fn plan_renumber(
        &self,
        drawing_rev: DrawingRevId,
        mode: RenumberMode,
    ) -> Result<Vec<RenumberChange>, RegisterError> {
        let rev = self.revs.get(&drawing_rev).ok_or(RegisterError::NotFound("drawing revision has no balloons"))?;
        let on_rev = |balloon: BalloonId| -> Result<u32, RegisterError> {
            match self.balloons.get(&balloon) {
                Some(b) if b.drawing_rev == drawing_rev => Ok(b.number),
                _ => Err(RegisterError::NotFound("balloon on drawing revision")),
            }
        };
        let mut changes = Vec::new();
        match mode {
            RenumberMode::Set { balloon, number } => {
                let old = on_rev(balloon)?;
                if old != number {
                    if rev.by_number.contains_key(&number) {
                        return Err(RegisterError::BalloonNumberTaken { number });
                    }
                    changes.push(RenumberChange { balloon, old, new: number });
                }
            }
            RenumberMode::Insert { balloon, number } => {
                let old = on_rev(balloon)?;
                for (&n, &other) in rev.by_number.range(number..) {
                    if other == balloon {
                        continue;
                    }
                    let new = n.checked_add(1).ok_or(RegisterError::BadParams("balloon number overflow"))?;
                    changes.push(RenumberChange { balloon: other, old: n, new });
                }
                if old != number {
                    changes.push(RenumberChange { balloon, old, new: number });
                }
            }
            RenumberMode::Compact => {
                let mut new = 0u32;
                for (&old, &balloon) in &rev.by_number {
                    // At most u32::MAX numbers exist, so `new` cannot overflow.
                    new = new.saturating_add(1);
                    if old != new {
                        changes.push(RenumberChange { balloon, old, new });
                    }
                }
            }
        }
        changes.sort_by_key(|c| c.new);
        Ok(changes)
    }

    // ------------------------------------------------------------------ commits (checked)

    /// Inserts a checked balloon; mode insert first moves numbers at or above it up by one,
    /// in descending order so the number index never holds two balloons for one number.
    pub(super) fn commit_add(&mut self, balloon: Balloon, insert: bool) {
        let rev = self.revs.entry(balloon.drawing_rev).or_default();
        if insert {
            let shifted: Vec<(u32, BalloonId)> =
                rev.by_number.range(balloon.number..).rev().map(|(&n, &id)| (n, id)).collect();
            for (n, id) in shifted {
                let new = n.saturating_add(1);
                rev.by_number.remove(&n);
                rev.by_number.insert(new, id);
                if let Some(b) = self.balloons.get_mut(&id) {
                    b.number = new;
                }
            }
        }
        rev.by_number.insert(balloon.number, balloon.id);
        rev.by_char.insert(balloon.characteristic.clone(), balloon.id);
        self.balloons.insert(balloon.id, balloon);
    }

    /// Applies planned renumber changes as one step: all old entries out, then all new in.
    pub(super) fn commit_renumber(&mut self, drawing_rev: DrawingRevId, changes: &[RenumberChange]) {
        let Some(rev) = self.revs.get_mut(&drawing_rev) else { return };
        for c in changes {
            rev.by_number.remove(&c.old);
        }
        for c in changes {
            rev.by_number.insert(c.new, c.balloon);
            if let Some(b) = self.balloons.get_mut(&c.balloon) {
                b.number = c.new;
            }
        }
    }

    pub(super) fn commit_move(&mut self, moved: Balloon) {
        self.balloons.insert(moved.id, moved);
    }

    pub(super) fn commit_relink(&mut self, relinked: Balloon) {
        if let Some(old) = self.balloons.get(&relinked.id) {
            if let Some(rev) = self.revs.get_mut(&old.drawing_rev) {
                rev.by_char.remove(&old.characteristic);
                rev.by_char.insert(relinked.characteristic.clone(), relinked.id);
            }
        }
        self.balloons.insert(relinked.id, relinked);
    }

    pub(super) fn commit_remove(&mut self, balloon: BalloonId) {
        if let Some(b) = self.balloons.remove(&balloon) {
            if let Some(rev) = self.revs.get_mut(&b.drawing_rev) {
                rev.by_number.remove(&b.number);
                rev.by_char.remove(&b.characteristic);
            }
        }
        self.retired_ids.insert(balloon);
    }

    pub(super) fn is_used_or_retired(&self, id: BalloonId) -> bool {
        self.balloons.contains_key(&id) || self.retired_ids.contains(&id)
    }

    pub(super) fn push_event(&mut self, event: BalloonEvent) {
        self.journal.push(event);
    }
}

/// Checks that `sheet` is on `drawing_rev`, is ready, and holds every point (`REQ-008`).
fn check_points(
    sheets: &dyn SheetInfo,
    drawing_rev: DrawingRevId,
    sheet: SheetId,
    points: &[SheetPoint],
) -> Result<(), RegisterError> {
    if sheets.revision(sheet) != Some(drawing_rev) {
        return Err(RegisterError::NotFound("sheet on drawing revision"));
    }
    let (width, height) = sheets.bounds(sheet).ok_or(RegisterError::SheetNotReady)?;
    for p in points {
        if !p.inside(width, height) {
            return Err(RegisterError::BalloonOffSheet { x: p.x, y: p.y, width, height });
        }
    }
    Ok(())
}

/// The canonical result value of a successful add, move or relink (a [`Balloon`]).
pub(super) fn balloon_digest_matches(b: &Balloon, want: &fid_types::Digest) -> bool {
    canonical_digest(&b.to_value()).is_ok_and(|d| d == *want)
}

/// The canonical result value of a successful renumber.
pub(super) fn changes_digest_matches(changes: &[RenumberChange], want: &fid_types::Digest) -> bool {
    canonical_digest(&changes_value(changes)).is_ok_and(|d| d == *want)
}
