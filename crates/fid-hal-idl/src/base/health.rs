//! Device health state machine and closed transition table (spec node `HAL.IDL.BASE.HEALTH`).
//!
//! Defines the ten [`HealthState`] values, the [`Actor`] causing a transition (`Device` or
//! `Runtime`), the closed sorted static [`TRANSITIONS`] table, [`check`] returning
//! [`IllegalTransition`] for every unlisted triple, and the warm-up / acquisition helpers
//! ([`warmup_state`], [`acquisition_allowed`], [`acquisition_allowed_default`]).
//!
//! Safety invariant (REQ-002): no transition out of [`HealthState::SafetyStopped`] is allowed
//! with [`Actor::Runtime`]; the only exit is `SAFETY_STOPPED -> INITIALIZING` reported by
//! [`Actor::Device`].

use crate::stamp::WarmupState;
use core::fmt;

/// The ten health states every HAL device reports (`HAL.IDL.BASE.HEALTH`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HealthState {
    Offline,
    Connecting,
    Initializing,
    WarmingUp,
    Ready,
    Busy,
    Fault,
    SafetyStopped,
    Maintenance,
    Interrupted,
}

impl HealthState {
    /// Every health state, in declaration order.
    pub const ALL: [HealthState; 10] = [
        HealthState::Offline,
        HealthState::Connecting,
        HealthState::Initializing,
        HealthState::WarmingUp,
        HealthState::Ready,
        HealthState::Busy,
        HealthState::Fault,
        HealthState::SafetyStopped,
        HealthState::Maintenance,
        HealthState::Interrupted,
    ];

    /// Canonical wire string.
    pub const fn as_str(self) -> &'static str {
        match self {
            HealthState::Offline => "OFFLINE",
            HealthState::Connecting => "CONNECTING",
            HealthState::Initializing => "INITIALIZING",
            HealthState::WarmingUp => "WARMING_UP",
            HealthState::Ready => "READY",
            HealthState::Busy => "BUSY",
            HealthState::Fault => "FAULT",
            HealthState::SafetyStopped => "SAFETY_STOPPED",
            HealthState::Maintenance => "MAINTENANCE",
            HealthState::Interrupted => "INTERRUPTED",
        }
    }

    /// Parses the exact uppercase wire name.
    pub fn parse(s: &str) -> Option<HealthState> {
        match s {
            "OFFLINE" => Some(HealthState::Offline),
            "CONNECTING" => Some(HealthState::Connecting),
            "INITIALIZING" => Some(HealthState::Initializing),
            "WARMING_UP" => Some(HealthState::WarmingUp),
            "READY" => Some(HealthState::Ready),
            "BUSY" => Some(HealthState::Busy),
            "FAULT" => Some(HealthState::Fault),
            "SAFETY_STOPPED" => Some(HealthState::SafetyStopped),
            "MAINTENANCE" => Some(HealthState::Maintenance),
            "INTERRUPTED" => Some(HealthState::Interrupted),
            _ => None,
        }
    }
}

impl fmt::Display for HealthState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who initiated a health state transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Actor {
    Device,
    Runtime,
}

impl Actor {
    pub const ALL: [Actor; 2] = [Actor::Device, Actor::Runtime];

    pub const fn as_str(self) -> &'static str {
        match self {
            Actor::Device => "Device",
            Actor::Runtime => "Runtime",
        }
    }

    pub fn parse(s: &str) -> Option<Actor> {
        match s {
            "Device" => Some(Actor::Device),
            "Runtime" => Some(Actor::Runtime),
            _ => None,
        }
    }
}

impl fmt::Display for Actor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One allowed transition entry in the closed table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Transition {
    pub from: HealthState,
    pub to: HealthState,
    pub by: Actor,
}

/// Refusal returned by [`check`] for any `(from, to, by)` triple not in [`TRANSITIONS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IllegalTransition {
    pub from: HealthState,
    pub to: HealthState,
    pub by: Actor,
}

impl IllegalTransition {
    pub const fn code(&self) -> &'static str {
        "illegal_transition"
    }
}

impl fmt::Display for IllegalTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "illegal_transition: {} -> {} by {}", self.from, self.to, self.by)
    }
}

impl std::error::Error for IllegalTransition {}

const fn t(from: HealthState, to: HealthState, by: Actor) -> Transition {
    Transition { from, to, by }
}

/// The closed, strictly ascending static table of allowed transitions (spec Design).
pub const TRANSITIONS: &[Transition] = &[
    // From OFFLINE
    t(HealthState::Offline, HealthState::Connecting, Actor::Runtime),
    t(HealthState::Offline, HealthState::Maintenance, Actor::Device),
    t(HealthState::Offline, HealthState::Maintenance, Actor::Runtime),
    // From CONNECTING
    t(HealthState::Connecting, HealthState::Offline, Actor::Runtime),
    t(HealthState::Connecting, HealthState::Initializing, Actor::Device),
    t(HealthState::Connecting, HealthState::Fault, Actor::Device),
    t(HealthState::Connecting, HealthState::Fault, Actor::Runtime),
    t(HealthState::Connecting, HealthState::Maintenance, Actor::Device),
    t(HealthState::Connecting, HealthState::Interrupted, Actor::Runtime),
    // From INITIALIZING
    t(HealthState::Initializing, HealthState::WarmingUp, Actor::Device),
    t(HealthState::Initializing, HealthState::Ready, Actor::Device),
    t(HealthState::Initializing, HealthState::Fault, Actor::Device),
    t(HealthState::Initializing, HealthState::Fault, Actor::Runtime),
    t(HealthState::Initializing, HealthState::SafetyStopped, Actor::Device),
    t(HealthState::Initializing, HealthState::Maintenance, Actor::Device),
    t(HealthState::Initializing, HealthState::Interrupted, Actor::Runtime),
    // From WARMING_UP
    t(HealthState::WarmingUp, HealthState::Ready, Actor::Device),
    t(HealthState::WarmingUp, HealthState::Fault, Actor::Device),
    t(HealthState::WarmingUp, HealthState::Fault, Actor::Runtime),
    t(HealthState::WarmingUp, HealthState::SafetyStopped, Actor::Device),
    t(HealthState::WarmingUp, HealthState::Maintenance, Actor::Device),
    t(HealthState::WarmingUp, HealthState::Interrupted, Actor::Runtime),
    // From READY
    t(HealthState::Ready, HealthState::Offline, Actor::Runtime),
    t(HealthState::Ready, HealthState::Busy, Actor::Device),
    t(HealthState::Ready, HealthState::Fault, Actor::Device),
    t(HealthState::Ready, HealthState::Fault, Actor::Runtime),
    t(HealthState::Ready, HealthState::SafetyStopped, Actor::Device),
    t(HealthState::Ready, HealthState::Maintenance, Actor::Device),
    t(HealthState::Ready, HealthState::Interrupted, Actor::Runtime),
    // From BUSY
    t(HealthState::Busy, HealthState::Ready, Actor::Device),
    t(HealthState::Busy, HealthState::Fault, Actor::Device),
    t(HealthState::Busy, HealthState::Fault, Actor::Runtime),
    t(HealthState::Busy, HealthState::SafetyStopped, Actor::Device),
    t(HealthState::Busy, HealthState::Interrupted, Actor::Runtime),
    // From FAULT
    t(HealthState::Fault, HealthState::Offline, Actor::Runtime),
    t(HealthState::Fault, HealthState::SafetyStopped, Actor::Device),
    t(HealthState::Fault, HealthState::Maintenance, Actor::Device),
    // From SAFETY_STOPPED (REQ-002: device-reported reset to INITIALIZING only)
    t(HealthState::SafetyStopped, HealthState::Initializing, Actor::Device),
    // From MAINTENANCE
    t(HealthState::Maintenance, HealthState::Offline, Actor::Device),
    t(HealthState::Maintenance, HealthState::Offline, Actor::Runtime),
    t(HealthState::Maintenance, HealthState::SafetyStopped, Actor::Device),
    // From INTERRUPTED (REQ-004: resume to CONNECTING or close to OFFLINE by Runtime)
    t(HealthState::Interrupted, HealthState::Offline, Actor::Runtime),
    t(HealthState::Interrupted, HealthState::Connecting, Actor::Runtime),
];

/// Checks whether transitioning from `from` to `to` initiated by `by` is allowed by the
/// closed [`TRANSITIONS`] table (REQ-001..004).
pub fn check(from: HealthState, to: HealthState, by: Actor) -> Result<(), IllegalTransition> {
    let target = Transition { from, to, by };
    if TRANSITIONS.binary_search(&target).is_ok() {
        Ok(())
    } else {
        Err(IllegalTransition { from, to, by })
    }
}

/// Maps device health to the provenance stamp's [`WarmupState`] (REQ-005):
/// `WARMING_UP` -> `WarmupState::WarmingUp`; `READY` or `BUSY` -> `WarmupState::Ready`;
/// every other state returns `None`.
pub const fn warmup_state(health: HealthState) -> Option<WarmupState> {
    match health {
        HealthState::WarmingUp => Some(WarmupState::WarmingUp),
        HealthState::Ready | HealthState::Busy => Some(WarmupState::Ready),
        HealthState::Offline
        | HealthState::Connecting
        | HealthState::Initializing
        | HealthState::Fault
        | HealthState::SafetyStopped
        | HealthState::Maintenance
        | HealthState::Interrupted => None,
    }
}

/// Whether acquisition is permitted in `health` (REQ-005): `READY` and `BUSY` are always
/// allowed; `WARMING_UP` is allowed only when `package_allows_warming_up` is true; all other
/// states refuse acquisition.
pub const fn acquisition_allowed(health: HealthState, package_allows_warming_up: bool) -> bool {
    match health {
        HealthState::Ready | HealthState::Busy => true,
        HealthState::WarmingUp => package_allows_warming_up,
        HealthState::Offline
        | HealthState::Connecting
        | HealthState::Initializing
        | HealthState::Fault
        | HealthState::SafetyStopped
        | HealthState::Maintenance
        | HealthState::Interrupted => false,
    }
}

/// Default package acquisition check (refuses `WARMING_UP`; allows only `READY` and `BUSY`).
pub const fn acquisition_allowed_default(health: HealthState) -> bool {
    acquisition_allowed(health, false)
}
