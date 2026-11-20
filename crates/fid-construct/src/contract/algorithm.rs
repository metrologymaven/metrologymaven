//! Construction algorithm IDs (REQ-KRN.CONS-009, REQ-KRN.CONS.DECL-002).
//!
//! # Pinned grammar
//!
//! ```text
//! construct-id = "construct." op "." variant "@" num "." num "." num
//! op, variant  = [a-z] [a-z0-9_]*          (each at most 32 bytes)
//! num          = "0" | [1-9] [0-9]*        (a u32, no leading zero)
//! ```
//!
//! for example `construct.intersect.line_plane@1.0.0`. This is the `KRN.MAN.CARD` ID
//! `<module>.<feature>.<method>@<semver>` with module `construct`, feature the operation and
//! method the variant, so every construction algorithm can carry a spec card and run under
//! `fid_kci::evaluate`. The whole text is at most 108 bytes.

use core::fmt;

use fid_kci::AlgorithmId;

use super::error::ConstructError;
use super::id::name_ok;

/// The module segment of every construction algorithm ID.
pub const MODULE: &str = "construct";

/// A validated construction algorithm ID. The field is private, so every value passed the
/// grammar in the module documentation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ConstructAlgorithmId(AlgorithmId);

impl ConstructAlgorithmId {
    /// Builds `construct.<op>.<variant>@<major>.<minor>.<patch>`.
    /// [`ConstructError::InvalidDecl`] (`algorithm`) when `op` or `variant` breaks the grammar.
    pub fn new(op: &str, variant: &str, version: (u32, u32, u32)) -> Result<Self, ConstructError> {
        if !name_ok(op) || !name_ok(variant) {
            return Err(ConstructError::InvalidDecl { field: "algorithm" });
        }
        Ok(ConstructAlgorithmId(AlgorithmId {
            module: MODULE.to_string(),
            feature: op.to_string(),
            method: variant.to_string(),
            version,
        }))
    }

    /// Parses the text form exactly (module documentation).
    pub fn parse(s: &str) -> Result<Self, ConstructError> {
        let bad = ConstructError::InvalidDecl { field: "algorithm" };
        if s.len() > 108 {
            return Err(bad);
        }
        let id = AlgorithmId::parse(s).map_err(|_| bad)?;
        if id.module != MODULE {
            return Err(bad);
        }
        let out = ConstructAlgorithmId::new(&id.feature, &id.method, id.version)?;
        // Round trip: the text is the one canonical spelling.
        if out.to_string() != s {
            return Err(bad);
        }
        Ok(out)
    }

    /// The operation segment (for example `intersect`).
    pub fn op(&self) -> &str {
        &self.0.feature
    }

    /// The variant segment (for example `line_plane`).
    pub fn variant(&self) -> &str {
        &self.0.method
    }

    /// `(major, minor, patch)`.
    pub fn version(&self) -> (u32, u32, u32) {
        self.0.version
    }

    /// The `KRN.MAN.CARD` form, for spec-card lookup and `fid_kci::evaluate`.
    pub fn as_kci(&self) -> &AlgorithmId {
        &self.0
    }
}

impl fmt::Display for ConstructAlgorithmId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
