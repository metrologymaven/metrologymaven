//! The journal event of an adoption (REQ-AUTO.REC.ADOPT-003): the adopting person, the
//! adopted step IDs (or the script version digest) and the program or script digest at
//! adoption time, as canonical JSON (RFC 8785).
//!
//! ```text
//! {"branch":"working","command":"program.adopt_steps@1","human":"<id>","program":"<uuid>",
//!  "program_digest":"sha256:<hex>","steps":["<uuid>",...]}
//! {"branch":"working","command":"script.adopt_version@1","human":"<id>","script":"<id>",
//!  "version_digest":"sha256:<hex>"}
//! ```
//!
//! `steps` is in program document order. The reader is strict: unknown, missing or
//! mistyped keys, a branch other than `working` and bytes that are not canonical are refused.

use super::{ScriptId, ADOPT_STEPS_COMMAND, ADOPT_VERSION_COMMAND, WORKING_BRANCH};
use fid_canon::{parse_with_limits, to_canonical_bytes, CValue};
use fid_program::{Uuid7, MAX_STEPS};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Largest event, in bytes: one step ID is 39 bytes with quotes and comma.
pub const MAX_EVENT_BYTES: usize = 1024 + 40 * MAX_STEPS;
const MAX_EVENT_DEPTH: usize = 2;
/// Longest person identity, in bytes (as `fid_bus::principal::MAX_ID_BYTES`).
const MAX_HUMAN_BYTES: usize = 256;

/// One adoption, as journaled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdoptEvent {
    /// `program.adopt_steps@1`.
    Steps {
        /// The adopting person.
        human: String,
        /// The program.
        program: Uuid7,
        /// The adopted steps, in document order.
        steps: Vec<Uuid7>,
        /// The program digest the person reviewed (equal to the current digest).
        program_digest: Digest,
    },
    /// `script.adopt_version@1`.
    Version {
        /// The adopting person.
        human: String,
        /// The script.
        script: ScriptId,
        /// The adopted version digest.
        version_digest: Digest,
    },
}

fn text(s: &str) -> CValue {
    CValue::Str(s.to_owned())
}

/// Why event bytes were refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadEvent(pub &'static str);

impl AdoptEvent {
    /// The command ID the event records.
    pub fn command(&self) -> &'static str {
        match self {
            AdoptEvent::Steps { .. } => ADOPT_STEPS_COMMAND,
            AdoptEvent::Version { .. } => ADOPT_VERSION_COMMAND,
        }
    }

    /// The adopting person.
    pub fn human(&self) -> &str {
        match self {
            AdoptEvent::Steps { human, .. } | AdoptEvent::Version { human, .. } => human,
        }
    }

    /// The JSON value.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("branch".to_owned(), text(WORKING_BRANCH));
        o.insert("command".to_owned(), text(self.command()));
        o.insert("human".to_owned(), text(self.human()));
        match self {
            AdoptEvent::Steps { program, steps, program_digest, .. } => {
                o.insert("program".to_owned(), text(&program.to_string()));
                o.insert("program_digest".to_owned(), text(program_digest.as_str()));
                o.insert("steps".to_owned(), CValue::Arr(steps.iter().map(|s| text(&s.to_string())).collect()));
            }
            AdoptEvent::Version { script, version_digest, .. } => {
                o.insert("script".to_owned(), text(script.as_str()));
                o.insert("version_digest".to_owned(), text(version_digest.as_str()));
            }
        }
        CValue::Obj(o)
    }

    /// Canonical JSON bytes, no trailing newline.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        // Only strings and arrays of strings: canonicalisation cannot fail.
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }

    /// Reads recorded bytes; they must be the canonical form of a valid event.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<AdoptEvent, BadEvent> {
        let v = parse_with_limits(bytes, MAX_EVENT_BYTES, MAX_EVENT_DEPTH).map_err(|_| BadEvent("not JSON"))?;
        let CValue::Obj(o) = v else { return Err(BadEvent("not an object")) };
        let s = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(BadEvent("missing or mistyped key")),
        };
        if s("branch")? != WORKING_BRANCH {
            return Err(BadEvent("branch is not working"));
        }
        let human = s("human")?;
        if human.is_empty() || human.len() > MAX_HUMAN_BYTES {
            return Err(BadEvent("bad human"));
        }
        let human = human.to_owned();
        let digest = |k: &str| Digest::parse(s(k)?).map_err(|_| BadEvent("bad digest"));
        let e = match s("command")? {
            ADOPT_STEPS_COMMAND => {
                let program = Uuid7::parse(s("program")?).ok_or(BadEvent("bad program"))?;
                let Some(CValue::Arr(a)) = o.get("steps") else { return Err(BadEvent("missing steps")) };
                let steps = a
                    .iter()
                    .map(|x| match x {
                        CValue::Str(x) => Uuid7::parse(x).ok_or(BadEvent("bad step")),
                        _ => Err(BadEvent("bad step")),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if steps.is_empty() {
                    return Err(BadEvent("no steps"));
                }
                AdoptEvent::Steps { human, program, steps, program_digest: digest("program_digest")? }
            }
            ADOPT_VERSION_COMMAND => {
                let script = ScriptId::new(s("script")?).map_err(|_| BadEvent("bad script"))?;
                AdoptEvent::Version { human, script, version_digest: digest("version_digest")? }
            }
            _ => return Err(BadEvent("unknown command")),
        };
        // Strict: the bytes must be exactly what this event writes (no extra keys, canonical).
        if e.to_canonical_bytes().as_slice() != bytes {
            return Err(BadEvent("not the canonical form of an event"));
        }
        Ok(e)
    }
}
