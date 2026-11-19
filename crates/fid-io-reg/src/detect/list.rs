//! The R0 listing commands `import.list_formats@1` and `export.list_formats@1`
//! (REQ-IO.REG.DETECT-001), their journal record and exact-mode replay (AC-7).
//!
//! The result is an array of descriptor objects (see [`super::FormatDescriptor::to_value`])
//! sorted by format ID, wrapped in the shared `computed` result envelope (`fid-surface`),
//! so `fid import list-formats --json` and the MCP tool print the same canonical bytes.
//! The journal record holds the command ID, the `origin` parameter and the SHA-256 of the
//! canonical result; replay recomputes the listing and compares digests.

use super::{DetectError, OriginFilter, Registry};
use crate::roots::Direction;
use fid_canon::CValue;
use fid_replay::ReplayOutcome;
use fid_surface::Envelope;
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;

/// Command ID of the reader listing.
pub const IMPORT_LIST_FORMATS: &str = "import.list_formats@1";
/// Command ID of the writer listing.
pub const EXPORT_LIST_FORMATS: &str = "export.list_formats@1";
/// Largest journal record accepted by [`ListEvent::from_json`], in bytes.
pub const MAX_EVENT_JSON_BYTES: usize = 4096;

/// The command ID listing `direction`.
pub fn command_id(direction: Direction) -> &'static str {
    match direction {
        Direction::Import => IMPORT_LIST_FORMATS,
        Direction::Export => EXPORT_LIST_FORMATS,
    }
}

fn direction_of(command: &str) -> Option<Direction> {
    match command {
        IMPORT_LIST_FORMATS => Some(Direction::Import),
        EXPORT_LIST_FORMATS => Some(Direction::Export),
        _ => None,
    }
}

fn bad(field: &'static str, reason: &'static str) -> DetectError {
    DetectError::Invalid { field, reason }
}

/// The result array: every descriptor of `direction` passing `filter`, sorted by ID.
pub fn list_formats_value(reg: &Registry, direction: Direction, filter: OriginFilter) -> CValue {
    CValue::Arr(reg.list(direction, filter).into_iter().map(|d| d.to_value()).collect())
}

/// The journal record of one listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListEvent {
    /// `import.list_formats@1` or `export.list_formats@1`.
    pub command: CommandId,
    /// The `origin` parameter.
    pub origin: OriginFilter,
    /// SHA-256 of the canonical result array.
    pub result_sha256: Digest,
}

impl ListEvent {
    /// The record as JSON: `{"command", "params": {"origin"}, "result_sha256"}`.
    pub fn to_value(&self) -> CValue {
        let mut params = BTreeMap::new();
        params.insert("origin".to_string(), CValue::Str(self.origin.as_str().to_string()));
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(self.command.as_str().to_string()));
        o.insert("params".to_string(), CValue::Obj(params));
        o.insert("result_sha256".to_string(), CValue::Str(self.result_sha256.as_str().to_string()));
        CValue::Obj(o)
    }

    /// Canonical JSON bytes of [`ListEvent::to_value`].
    pub fn to_json(&self) -> Result<Vec<u8>, DetectError> {
        fid_canon::to_canonical_bytes(&self.to_value()).map_err(|_| bad("event", "not canonical JSON"))
    }

    /// Parses a record written by [`ListEvent::to_json`]. Strict: bounded input, no
    /// duplicate or unknown keys, a listing command ID and a valid digest.
    pub fn from_json(bytes: &[u8]) -> Result<ListEvent, DetectError> {
        let v = fid_canon::parse_with_limits(bytes, MAX_EVENT_JSON_BYTES, 4)
            .map_err(|_| bad("event", "not acceptable JSON"))?;
        let CValue::Obj(mut o) = v else { return Err(bad("event", "must be an object")) };
        let command = match o.remove("command") {
            Some(CValue::Str(s)) if direction_of(&s).is_some() => {
                CommandId::parse(&s).map_err(|_| bad("command", "invalid command ID"))?
            }
            _ => return Err(bad("command", "not a listing command")),
        };
        let origin = match o.remove("params") {
            Some(CValue::Obj(mut p)) => {
                let origin = match p.remove("origin") {
                    Some(CValue::Str(s)) => OriginFilter::from_wire(&s).ok_or_else(|| bad("origin", "unknown"))?,
                    _ => return Err(bad("origin", "must be a string")),
                };
                if !p.is_empty() {
                    return Err(bad("params", "unknown field"));
                }
                origin
            }
            _ => return Err(bad("params", "must be an object")),
        };
        let result_sha256 = match o.remove("result_sha256") {
            Some(CValue::Str(s)) => Digest::parse(&s).map_err(|_| bad("result_sha256", "invalid digest"))?,
            _ => return Err(bad("result_sha256", "must be a string")),
        };
        if !o.is_empty() {
            return Err(bad("event", "unknown field"));
        }
        Ok(ListEvent { command, origin, result_sha256 })
    }
}

/// The output of one listing command: the result envelope and the journal record.
#[derive(Debug, Clone, PartialEq)]
pub struct ListOutput {
    /// The `computed` envelope whose `result` is the descriptor array.
    pub envelope: Envelope,
    /// The journal record.
    pub event: ListEvent,
}

fn digest_of(result: &CValue) -> Result<Digest, DetectError> {
    let d = fid_canon::digest(result).map_err(|_| bad("result", "not canonical JSON"))?;
    Digest::parse(&d.to_string()).map_err(|_| bad("result", "digest"))
}

/// Runs `import.list_formats@1` ([`Direction::Import`]) or `export.list_formats@1`
/// ([`Direction::Export`]). Read-only (R0), D0.
pub fn list_formats(reg: &Registry, direction: Direction, filter: OriginFilter) -> Result<ListOutput, DetectError> {
    let id = command_id(direction);
    let result = list_formats_value(reg, direction, filter);
    let result_sha256 = digest_of(&result)?;
    let command = CommandId::parse(id).map_err(|_| bad("command", "invalid command ID"))?;
    Ok(ListOutput {
        envelope: Envelope::computed(id, result),
        event: ListEvent { command, origin: filter, result_sha256 },
    })
}

/// Exact-mode replay of a listing against `reg`: `IDENTICAL` when the recomputed result has
/// the recorded digest, `DIVERGENT` otherwise. A record naming another command is
/// `NOT_REPLAYABLE`.
pub fn replay(event: &ListEvent, reg: &Registry) -> Result<ReplayOutcome, DetectError> {
    let Some(direction) = direction_of(event.command.as_str()) else {
        return Ok(ReplayOutcome::NotReplayable);
    };
    let out = list_formats(reg, direction, event.origin)?;
    Ok(if out.event == *event { ReplayOutcome::Identical } else { ReplayOutcome::Divergent })
}
