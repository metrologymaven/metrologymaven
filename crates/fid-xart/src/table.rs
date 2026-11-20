//! The digest table one artifact produces for one call set on one host (spec Design:
//! `DigestTable { call_set_digest, artifact_sha256, rows }`).
//!
//! Canonical JSON, keys in byte order, every value a string:
//!
//! ```text
//! {"artifact":"<label>","artifact_sha256":"sha256:..","call_set_digest":"sha256:..",
//!  "host":"<label>","kind":"wasm|native","rows":[{"algorithm":"<id>","call":"<index>",
//!  "exit":"0|1","name":"<call name>","result_digest":"sha256:.."}],"schema":"fid-xart-table@1"}
//! ```
//!
//! `result_digest` is the SHA-256 of the artifact's whole stdout for the call: the canonical
//! `KernelResult` envelope (exit 0) or the `{"error":"<code>"}` object (exit 1). Two artifacts
//! agree on a call exactly when they wrote the same bytes with the same exit code.

use fid_canon::CValue;
use fid_kci::kci::json::Json;
use fid_kci::{AlgorithmId, Digest};

use crate::json::{self, obj, s};
use crate::{is_label, is_name, XartError};

/// The `schema` member of a digest table.
pub const TABLE_SCHEMA: &str = "fid-xart-table@1";
/// Largest digest table read.
pub const MAX_TABLE_BYTES: usize = 64 * 1024 * 1024;

/// Which kind of artifact produced a table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// The WASM reference artifact under the pinned runner.
    Wasm,
    /// A native performance artifact.
    Native,
}

impl Kind {
    /// Wire form.
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Wasm => "wasm",
            Kind::Native => "native",
        }
    }

    /// Parses the wire form.
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "wasm" => Some(Kind::Wasm),
            "native" => Some(Kind::Native),
            _ => None,
        }
    }
}

/// One call's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// 0-based index in the call set.
    pub call: usize,
    /// The call's name in the call set.
    pub name: String,
    /// The call's algorithm ID.
    pub algorithm: String,
    /// 0 for a result, 1 for a kernel error.
    pub exit: u8,
    /// SHA-256 of the artifact's stdout.
    pub result_digest: Digest,
}

/// The digest table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DigestTable {
    /// Artifact label (for example `kernel-wasm` or `native-release`).
    pub artifact: String,
    /// SHA-256 of the artifact file.
    pub artifact_sha256: Digest,
    /// SHA-256 of the call-set file.
    pub call_set_digest: Digest,
    /// Host label from the matrix.
    pub host: String,
    /// WASM or native.
    pub kind: Kind,
    /// One row per call, in call-set order.
    pub rows: Vec<Row>,
}

const TABLE_KEYS: [&str; 7] = ["artifact", "artifact_sha256", "call_set_digest", "host", "kind", "rows", "schema"];
const ROW_KEYS: [&str; 5] = ["algorithm", "call", "exit", "name", "result_digest"];

/// True for an algorithm ID in its exact `AlgorithmId` form.
pub(crate) fn is_algorithm(s: &str) -> bool {
    AlgorithmId::parse(s).is_ok_and(|id| id.to_string() == s)
}

impl DigestTable {
    /// Canonical JSON bytes (no trailing newline).
    pub fn to_json(&self) -> Result<Vec<u8>, XartError> {
        let rows = self
            .rows
            .iter()
            .map(|r| {
                obj(vec![
                    ("algorithm", s(&r.algorithm)),
                    ("call", s(&r.call.to_string())),
                    ("exit", s(&r.exit.to_string())),
                    ("name", s(&r.name)),
                    ("result_digest", s(r.result_digest.as_str())),
                ])
            })
            .collect();
        json::write(&obj(vec![
            ("artifact", s(&self.artifact)),
            ("artifact_sha256", s(self.artifact_sha256.as_str())),
            ("call_set_digest", s(self.call_set_digest.as_str())),
            ("host", s(&self.host)),
            ("kind", s(self.kind.as_str())),
            ("rows", Json::Arr(rows)),
            ("schema", s(TABLE_SCHEMA)),
        ]))
        .map_err(XartError::Table)
    }

    /// Parses a table. Only the canonical bytes of a valid table are accepted, so a table that
    /// was edited by hand (re-indented, reordered, extra members) is refused.
    pub fn parse(bytes: &[u8]) -> Result<DigestTable, XartError> {
        let v = json::parse_canonical(bytes, MAX_TABLE_BYTES).map_err(XartError::Table)?;
        Self::from_value(&v).map_err(XartError::Table)
    }

    fn from_value(v: &CValue) -> Result<DigestTable, String> {
        let m = json::object(v, &TABLE_KEYS, &[])?;
        if json::string(m, "schema")? != TABLE_SCHEMA {
            return Err("unknown_schema".to_string());
        }
        let artifact = json::string(m, "artifact")?;
        let host = json::string(m, "host")?;
        if !is_label(artifact) || !is_label(host) {
            return Err("bad_label".to_string());
        }
        let kind = Kind::parse(json::string(m, "kind")?).ok_or("bad_kind")?;
        let mut rows = Vec::new();
        for (i, r) in json::array(m, "rows")?.iter().enumerate() {
            let r = json::object(r, &ROW_KEYS, &[])?;
            let name = json::string(r, "name")?;
            let algorithm = json::string(r, "algorithm")?;
            if json::decimal(json::string(r, "call")?) != Some(i) {
                return Err(format!("row {i}: call index out of order"));
            }
            if !is_name(name) || !is_algorithm(algorithm) {
                return Err(format!("row {i}: bad name or algorithm"));
            }
            let exit = match json::string(r, "exit")? {
                "0" => 0,
                "1" => 1,
                _ => return Err(format!("row {i}: bad exit")),
            };
            rows.push(Row {
                call: i,
                name: name.to_string(),
                algorithm: algorithm.to_string(),
                exit,
                result_digest: json::digest(r, "result_digest")?,
            });
        }
        if rows.is_empty() {
            return Err("no_rows".to_string());
        }
        Ok(DigestTable {
            artifact: artifact.to_string(),
            artifact_sha256: json::digest(m, "artifact_sha256")?,
            call_set_digest: json::digest(m, "call_set_digest")?,
            host: host.to_string(),
            kind,
            rows,
        })
    }

    /// True when both tables list the same calls (index, name and algorithm) in the same order.
    pub fn same_calls(&self, other: &DigestTable) -> bool {
        self.rows.len() == other.rows.len()
            && self
                .rows
                .iter()
                .zip(&other.rows)
                .all(|(a, b)| a.call == b.call && a.name == b.name && a.algorithm == b.algorithm)
    }
}
