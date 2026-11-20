//! The `construct.*` journal event (REQ-KRN.CONS.DECL-006, REQ-KRN.CONS-010).
//!
//! When a `construct.<verb_object>@<major>` command commits, the journal gets one event
//! whose type is the command ID and whose payload holds the four journal fields: algorithm
//! ID, input element IDs (with roles, declared order), parameter digest and output digest.
//! The kernel only shapes the event; `CORE.JRN` appends it.
//!
//! # Pinned payload
//!
//! RFC 8785 canonical JSON, keys sorted (the bytes `fid_canon::to_canonical_bytes` writes
//! for the same value):
//!
//! ```text
//! {"algorithm":"construct.<op>.<variant>@<semver>",
//!  "inputs":[{"id":"<uuid>","role":"<role>"},...],
//!  "output_digest":"sha256:<hex>","params_digest":"sha256:<hex>"}
//! ```

use fid_types::{CommandId, Digest};

use super::algorithm::ConstructAlgorithmId;
use super::decl::refs_json;
use super::id::ElementRef;
use super::json::{self, J};

/// One `construct.*` journal event. Fields are private; only
/// [`super::Evaluation::event`] builds one, from a successful evaluation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConstructEvent {
    command: CommandId,
    algorithm: ConstructAlgorithmId,
    inputs: Vec<ElementRef>,
    params_digest: Digest,
    output_digest: Digest,
}

impl ConstructEvent {
    pub(crate) fn new(
        command: CommandId,
        algorithm: ConstructAlgorithmId,
        inputs: Vec<ElementRef>,
        params_digest: Digest,
        output_digest: Digest,
    ) -> ConstructEvent {
        ConstructEvent { command, algorithm, inputs, params_digest, output_digest }
    }

    /// The event type: the committed command ID (`construct.<verb_object>@<major>`).
    pub fn event_type(&self) -> &str {
        self.command.as_str()
    }

    /// The algorithm that produced the element.
    pub fn algorithm(&self) -> &ConstructAlgorithmId {
        &self.algorithm
    }

    /// The input references, in declared order.
    pub fn inputs(&self) -> &[ElementRef] {
        &self.inputs
    }

    /// The parameter digest.
    pub fn params_digest(&self) -> &Digest {
        &self.params_digest
    }

    /// The output digest.
    pub fn output_digest(&self) -> &Digest {
        &self.output_digest
    }

    /// The canonical payload bytes (module documentation).
    pub fn payload_bytes(&self) -> Vec<u8> {
        json::write(&J::O(vec![
            ("algorithm", J::S(self.algorithm.to_string())),
            ("inputs", refs_json(&self.inputs)),
            ("output_digest", json::s(self.output_digest.as_str())),
            ("params_digest", json::s(self.params_digest.as_str())),
        ]))
    }
}
