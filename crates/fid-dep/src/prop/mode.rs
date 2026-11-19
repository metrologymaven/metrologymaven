//! The project's recompute mode and `project.set_recompute_mode@1` (W2, D0, undo
//! `head_move`) (REQ-003).

use super::PropError;
use fid_canon::CValue;
use fid_registry::{CommandDef, DefError};
use std::collections::BTreeMap;

/// Command ID of the mode setter.
pub const SET_RECOMPUTE_MODE: &str = "project.set_recompute_mode@1";
/// Journal event type of a [`RecomputeModeSet`] event.
pub const RECOMPUTE_MODE_SET_EVENT: &str = "project.recompute_mode_set@1";
const NAME: &str = "project.set_recompute_mode";
const LIFECYCLE_SINCE: &str = "2026-11-19";

/// Whether a marking commit also submits a recompute (`auto`) or only marks (`manual`).
/// A new project is `auto` (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum RecomputeMode {
    #[default]
    Auto,
    Manual,
}

impl RecomputeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            RecomputeMode::Auto => "auto",
            RecomputeMode::Manual => "manual",
        }
    }

    /// Parses the exact lowercase text form; anything else is `None`.
    pub fn parse(text: &str) -> Option<RecomputeMode> {
        match text {
            "auto" => Some(RecomputeMode::Auto),
            "manual" => Some(RecomputeMode::Manual),
            _ => None,
        }
    }

    /// The mode at a journal head: the last [`RecomputeModeSet`] event, or `auto` when
    /// there is none. Undo by `head_move` is replay up to the earlier head, so undoing the
    /// only set restores `auto`.
    pub fn replay<'a>(events: impl IntoIterator<Item = &'a RecomputeModeSet>) -> RecomputeMode {
        events.into_iter().last().map(|e| e.mode).unwrap_or_default()
    }

    /// The command result `{"mode": "auto" | "manual"}`.
    pub fn result_value(self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("mode".to_string(), CValue::Str(self.as_str().to_string()));
        CValue::Obj(o)
    }
}

/// Parameters of `project.set_recompute_mode@1`: `{ mode: "auto" | "manual" }`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetRecomputeMode {
    pub mode: RecomputeMode,
}

impl SetRecomputeMode {
    /// Reads the parameter object. Fails closed with `validation` on a non-object, an
    /// unknown key, a missing `mode`, a non-string or any value other than `auto`/`manual`.
    pub fn from_value(v: &CValue) -> Result<SetRecomputeMode, PropError> {
        let CValue::Obj(o) = v else { return Err(PropError::Validation("params is not an object")) };
        if o.keys().any(|k| k != "mode") {
            return Err(PropError::Validation("unknown parameter"));
        }
        let mode = match o.get("mode") {
            Some(CValue::Str(s)) => {
                RecomputeMode::parse(s).ok_or(PropError::Validation("mode is not auto or manual"))?
            }
            Some(_) => return Err(PropError::Validation("mode is not a string")),
            None => return Err(PropError::Validation("missing mode")),
        };
        Ok(SetRecomputeMode { mode })
    }
}

/// The event `project.set_recompute_mode@1` appends: `{"mode": "auto" | "manual"}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecomputeModeSet {
    pub mode: RecomputeMode,
}

impl RecomputeModeSet {
    pub fn to_value(&self) -> CValue {
        self.mode.result_value()
    }

    /// Reads an event written by [`RecomputeModeSet::to_value`]; fails closed (`bad_event`).
    pub fn from_value(v: &CValue) -> Result<RecomputeModeSet, PropError> {
        let CValue::Obj(o) = v else { return Err(PropError::BadEvent("not an object")) };
        if o.keys().any(|k| k != "mode") {
            return Err(PropError::BadEvent("unknown key"));
        }
        match o.get("mode") {
            Some(CValue::Str(s)) => {
                RecomputeMode::parse(s).map(|mode| RecomputeModeSet { mode }).ok_or(PropError::BadEvent("bad mode"))
            }
            Some(_) => Err(PropError::BadEvent("mode is not a string")),
            None => Err(PropError::BadEvent("missing mode")),
        }
    }
}

/// Runs `project.set_recompute_mode@1`: validates `params` and returns the event to journal
/// (one per call, also when the mode does not change). On error nothing is to be journaled.
/// The command result is [`RecomputeMode::result_value`] of the event's mode.
pub fn set_recompute_mode(params: &CValue) -> Result<RecomputeModeSet, PropError> {
    SetRecomputeMode::from_value(params).map(|p| RecomputeModeSet { mode: p.mode })
}

fn obj<const N: usize>(pairs: [(&str, CValue); N]) -> CValue {
    CValue::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_string())
}

fn strs(items: &[&str]) -> CValue {
    CValue::Arr(items.iter().map(|s| text(s)).collect())
}

fn mode_schema(description: &str) -> CValue {
    obj([("type", text("string")), ("enum", strs(&["auto", "manual"])), ("description", text(description))])
}

fn object_schema(description: &str) -> CValue {
    obj([
        ("type", text("object")),
        ("additionalProperties", CValue::Bool(false)),
        ("required", strs(&["mode"])),
        ("properties", obj([("mode", mode_schema(description))])),
    ])
}

/// The registry definition of `project.set_recompute_mode@1`, built through
/// [`CommandDef::from_value`] so every field rule of the registry applies.
pub fn set_command_def() -> Result<CommandDef, DefError> {
    CommandDef::from_value(obj([
        ("id", text(NAME)),
        ("major", CValue::Num(1.0)),
        ("title", text("Set recompute mode")),
        (
            "description",
            text(
                "Sets whether a change that marks dependents outdated also starts a recompute (auto) or only marks \
                 them (manual). A new project is auto.",
            ),
        ),
        ("params_schema", object_schema("The new recompute mode.")),
        ("result_schema", object_schema("The recompute mode now in effect.")),
        ("side_effect", text("W2")),
        ("mcp_projection", text("exposed")),
        ("determinism", text("D0")),
        ("idempotency", text("natural")),
        ("fs_effect", text("none")),
        ("undo", text("head_move")),
        ("preconditions", CValue::Arr(Vec::new())),
        ("effects", strs(&[RECOMPUTE_MODE_SET_EVENT])),
        ("algorithms", CValue::Arr(Vec::new())),
        ("exposure", strs(&["cli", "mcp"])),
        ("lifecycle", obj([("state", text("active")), ("since", text(LIFECYCLE_SINCE))])),
        ("human_only", CValue::Bool(false)),
        ("network", text("none")),
        ("web", text("available")),
        ("cli_bare", CValue::Bool(false)),
    ]))
}
