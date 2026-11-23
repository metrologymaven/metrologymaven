//! Exhaustive acceptance tests for `HAL.IDL.BASE.HEALTH` (`spec/hal/idl/base/health/NODE.md`):
//! AC-1 (all 10 x 10 x 2 = 200 triples against fixtures/hal/idl/base/health/table.json),
//! AC-2 (no runtime exit from SAFETY_STOPPED),
//! AC-3 (SAFETY_STOPPED -> INITIALIZING by Device is Ok),
//! AC-4 (MAINTENANCE entry rules),
//! AC-5 (warmup_state and acquisition_allowed rules).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse_with_limits, CValue};
use fid_hal_idl::base::health::{
    acquisition_allowed, acquisition_allowed_default, check, warmup_state, Actor, HealthState, IllegalTransition,
    Transition, TRANSITIONS,
};
use fid_hal_idl::stamp::WarmupState;
use std::collections::BTreeSet;

const TABLE_JSON: &str = include_str!("../../../fixtures/hal/idl/base/health/table.json");

fn load_allowed_from_fixture() -> BTreeSet<Transition> {
    let v = parse_with_limits(TABLE_JSON.as_bytes(), 64 * 1024, 8).unwrap();
    let CValue::Obj(root) = v else { panic!("root object") };
    let CValue::Arr(allowed) = root.get("allowed").unwrap() else { panic!("allowed array") };

    let mut set = BTreeSet::new();
    for item in allowed {
        let CValue::Obj(o) = item else { panic!("entry") };
        let CValue::Str(f) = o.get("from").unwrap() else { panic!("from") };
        let CValue::Str(t) = o.get("to").unwrap() else { panic!("to") };
        let CValue::Str(b) = o.get("by").unwrap() else { panic!("by") };
        let tr = Transition {
            from: HealthState::parse(f).expect("valid from"),
            to: HealthState::parse(t).expect("valid to"),
            by: Actor::parse(b).expect("valid actor"),
        };
        assert!(set.insert(tr), "duplicate transition in fixture: {tr:?}");
    }
    set
}

/// AC-1 + REQ-001: Exhaustive check of all 10 × 10 × 2 = 200 `(from, to, by)` triples against
/// `fixtures/hal/idl/base/health/table.json`, plus verification that `TRANSITIONS` is strictly
/// ascending (required for binary search).
#[test]
fn ac1_exhaustive_200_triples_match_fixture_table() {
    for pair in TRANSITIONS.windows(2) {
        assert!(pair[0] < pair[1], "TRANSITIONS not strictly sorted: {:?} >= {:?}", pair[0], pair[1]);
    }
    let allowed = load_allowed_from_fixture();
    assert_eq!(allowed.len(), TRANSITIONS.len());

    let mut checked = 0usize;
    let mut ok_count = 0usize;
    for from in HealthState::ALL {
        for to in HealthState::ALL {
            for by in Actor::ALL {
                checked += 1;
                let tr = Transition { from, to, by };
                let res = check(from, to, by);
                if allowed.contains(&tr) {
                    assert_eq!(res, Ok(()), "expected Ok for {tr:?}");
                    ok_count += 1;
                } else {
                    assert_eq!(res, Err(IllegalTransition { from, to, by }), "expected IllegalTransition for {tr:?}");
                }
            }
        }
    }
    assert_eq!(checked, 200);
    assert_eq!(ok_count, allowed.len());
}

/// AC-2 + AC-3 + REQ-002: No runtime transition ever leaves `SAFETY_STOPPED`; the sole exit is
/// `(SAFETY_STOPPED, INITIALIZING, Device)`.
#[test]
fn ac2_ac3_safety_stopped_exit_rules() {
    assert_eq!(
        check(HealthState::SafetyStopped, HealthState::Ready, Actor::Runtime),
        Err(IllegalTransition { from: HealthState::SafetyStopped, to: HealthState::Ready, by: Actor::Runtime })
    );
    assert_eq!(
        check(HealthState::SafetyStopped, HealthState::Offline, Actor::Runtime),
        Err(IllegalTransition { from: HealthState::SafetyStopped, to: HealthState::Offline, by: Actor::Runtime })
    );
    for to in HealthState::ALL {
        assert!(
            check(HealthState::SafetyStopped, to, Actor::Runtime).is_err(),
            "Runtime must never exit SAFETY_STOPPED to {to}"
        );
        if to == HealthState::Initializing {
            assert_eq!(check(HealthState::SafetyStopped, to, Actor::Device), Ok(()));
        } else {
            assert!(
                check(HealthState::SafetyStopped, to, Actor::Device).is_err(),
                "Device may exit SAFETY_STOPPED only to INITIALIZING, not {to}"
            );
        }
    }
}

/// AC-4 + REQ-003 + REQ-004: `MAINTENANCE` entry/exit rules and `INTERRUPTED` runtime-only rules.
#[test]
fn ac4_maintenance_and_interrupted_rules() {
    assert!(check(HealthState::Ready, HealthState::Maintenance, Actor::Runtime).is_err());
    assert_eq!(check(HealthState::Offline, HealthState::Maintenance, Actor::Runtime), Ok(()));
    assert!(check(HealthState::Busy, HealthState::Maintenance, Actor::Device).is_err());
    assert!(check(HealthState::SafetyStopped, HealthState::Maintenance, Actor::Device).is_err());
    assert_eq!(check(HealthState::Ready, HealthState::Maintenance, Actor::Device), Ok(()));
    assert_eq!(check(HealthState::Maintenance, HealthState::Offline, Actor::Device), Ok(()));
    assert_eq!(check(HealthState::Maintenance, HealthState::Offline, Actor::Runtime), Ok(()));

    // REQ-004: INTERRUPTED entered only by Runtime, left only to CONNECTING or OFFLINE by Runtime.
    for from in HealthState::ALL {
        assert!(check(from, HealthState::Interrupted, Actor::Device).is_err());
    }
    assert_eq!(check(HealthState::Interrupted, HealthState::Connecting, Actor::Runtime), Ok(()));
    assert_eq!(check(HealthState::Interrupted, HealthState::Offline, Actor::Runtime), Ok(()));
    assert!(check(HealthState::Interrupted, HealthState::Ready, Actor::Runtime).is_err());
    assert!(check(HealthState::Interrupted, HealthState::Offline, Actor::Device).is_err());
}

/// AC-5 + REQ-005: `warmup_state` and `acquisition_allowed` across all 10 health states.
#[test]
fn ac5_warmup_state_and_acquisition_allowed() {
    assert_eq!(warmup_state(HealthState::WarmingUp), Some(WarmupState::WarmingUp));
    assert_eq!(warmup_state(HealthState::Ready), Some(WarmupState::Ready));
    assert_eq!(warmup_state(HealthState::Busy), Some(WarmupState::Ready));
    for s in [
        HealthState::Offline,
        HealthState::Connecting,
        HealthState::Initializing,
        HealthState::Fault,
        HealthState::SafetyStopped,
        HealthState::Maintenance,
        HealthState::Interrupted,
    ] {
        assert_eq!(warmup_state(s), None, "{s} must not have a warmup_state");
        assert!(!acquisition_allowed_default(s), "{s} must refuse default acquisition");
        assert!(!acquisition_allowed(s, true), "{s} must refuse acquisition even if warmup allowed");
    }
    assert!(acquisition_allowed_default(HealthState::Ready));
    assert!(acquisition_allowed_default(HealthState::Busy));
    assert!(!acquisition_allowed_default(HealthState::WarmingUp));
    assert!(acquisition_allowed(HealthState::WarmingUp, true));
}
