//! Exact replay of the balloon register's journal (`RPT.BALLOON.REG` AC-7).
//!
//! Replay never generates an ID: each add reads its balloon ID back from the event, and an
//! ID that is already live or retired is a divergence (`REQ-009`). Every event is re-checked
//! with the same rules as the handler, the number of an add is re-derived from the requested
//! number and compared, and the SHA-256 of each re-computed result must equal the event's
//! `result_digest`. The first event that fails stops replay with `DIVERGENT`.

use super::event::BalloonEvent;
use super::model::Balloon;
use super::store::{balloon_digest_matches, changes_digest_matches, BalloonRegister, MoveBalloonParams, RegisterEnv};
use fid_replay::ReplayOutcome;

/// Result of [`replay_exact`].
#[derive(Debug, Clone)]
pub struct BalloonReplayRun {
    /// `Identical` when every event replayed to the same result.
    pub outcome: ReplayOutcome,
    /// Index of the first event that did not replay, if any.
    pub first_mismatch: Option<usize>,
    /// The register rebuilt from the events up to (not including) the first mismatch.
    pub register: BalloonRegister,
}

/// Replays `events` in order into a fresh register against `env`.
pub fn replay_exact(events: &[BalloonEvent], env: &RegisterEnv<'_>) -> BalloonReplayRun {
    let mut reg = BalloonRegister::new();
    for (idx, ev) in events.iter().enumerate() {
        if !replay_one(&mut reg, env, ev) {
            return BalloonReplayRun { outcome: ReplayOutcome::Divergent, first_mismatch: Some(idx), register: reg };
        }
        reg.push_event(ev.clone());
    }
    BalloonReplayRun { outcome: ReplayOutcome::Identical, first_mismatch: None, register: reg }
}

fn replay_one(reg: &mut BalloonRegister, env: &RegisterEnv<'_>, ev: &BalloonEvent) -> bool {
    match ev {
        BalloonEvent::Add(a) => {
            if reg.is_used_or_retired(a.id) {
                return false;
            }
            let Ok(number) =
                reg.check_add(env, a.drawing_rev, a.sheet, &a.characteristic, a.anchor, a.label, a.requested, a.insert)
            else {
                return false;
            };
            let balloon = Balloon {
                id: a.id,
                number,
                drawing_rev: a.drawing_rev,
                sheet: a.sheet,
                characteristic: a.characteristic.clone(),
                anchor: a.anchor,
                label: a.label,
                zone: a.zone.clone(),
            };
            if number != a.number || !balloon_digest_matches(&balloon, &a.result_digest) {
                return false;
            }
            reg.commit_add(balloon, a.insert);
            true
        }
        BalloonEvent::Move { balloon, sheet, anchor, label, result_digest } => {
            let p = MoveBalloonParams { balloon: *balloon, anchor: *anchor, label: *label, sheet: *sheet };
            match reg.check_move(env, &p) {
                Ok(moved) if balloon_digest_matches(&moved, result_digest) => {
                    reg.commit_move(moved);
                    true
                }
                _ => false,
            }
        }
        BalloonEvent::Renumber { drawing_rev, mode, result_digest } => match reg.plan_renumber(*drawing_rev, *mode) {
            Ok(changes) if changes_digest_matches(&changes, result_digest) => {
                reg.commit_renumber(*drawing_rev, &changes);
                true
            }
            _ => false,
        },
        BalloonEvent::Relink { balloon, characteristic, result_digest } => {
            match reg.check_relink(env, *balloon, characteristic) {
                Ok(relinked) if balloon_digest_matches(&relinked, result_digest) => {
                    reg.commit_relink(relinked);
                    true
                }
                _ => false,
            }
        }
        BalloonEvent::Remove { balloon, result_digest } => {
            let ok = reg.get(*balloon).is_some_and(|b| balloon_digest_matches(b, result_digest));
            if ok {
                reg.commit_remove(*balloon);
            }
            ok
        }
    }
}
