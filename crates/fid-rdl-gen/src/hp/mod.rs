//! High-precision arithmetic, rounding, Gauss-Newton refinement, seeded RNG and
//! [`GeneratorRecord`] (spec node `UNC.RDL.GEN.HPREC`).

pub mod decimal;
pub mod real;
pub mod refine;

use std::collections::BTreeMap;

use fid_canon::{digest as canon_digest, to_canonical_bytes, CValue};
use fid_num::rng::{Address, Cursor};
use fid_types::Digest;

pub use decimal::{format_decimal_sig, parse_decimal_hp, round16, Decimal16, ReferenceValue, MIN_REFERENCE_DIGITS};
pub use real::{Hp, PRECISION_BITS};
pub use refine::{hp_refine, tol_1e_50, HpCert, HpError, MAX_REFINE_ITERATIONS};

/// Metadata written into every tier-G generated dataset (REQ-UNC.RDL.GEN.HPREC-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratorRecord {
    pub family: String,
    pub method: String,
    pub tool_version: String,
    pub seed: u64,
    pub precision_bits: u32,
    pub digits: u32,
}

impl GeneratorRecord {
    /// Canonical RFC 8785 JSON value (`seed` written as decimal string so u64 survives JSON
    /// losslessly across every target, matching `fid_rdl::GeneratorRecord`).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("digits".to_string(), CValue::Num(f64::from(self.digits)));
        o.insert("family".to_string(), CValue::Str(self.family.clone()));
        o.insert("method".to_string(), CValue::Str(self.method.clone()));
        o.insert("precision_bits".to_string(), CValue::Num(f64::from(self.precision_bits)));
        o.insert("seed".to_string(), CValue::Str(self.seed.to_string()));
        o.insert("tool_version".to_string(), CValue::Str(self.tool_version.clone()));
        CValue::Obj(o)
    }

    /// Canonical UTF-8 RFC 8785 JSON bytes (byte-identical across x86-64, ARM64 and wasm32).
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, HpError> {
        to_canonical_bytes(&self.to_value()).map_err(|_| HpError::Domain("cannot canonicalise GeneratorRecord"))
    }

    /// SHA-256 digest of [`Self::to_canonical_bytes`].
    pub fn digest(&self) -> Result<Digest, HpError> {
        let d = canon_digest(&self.to_value()).map_err(|_| HpError::Domain("cannot hash GeneratorRecord"))?;
        Ok(Digest::from_bytes(d.0))
    }

    /// Converts into the `rdl-dataset/v1` [`fid_rdl::GeneratorRecord`] metadata block.
    pub fn to_rdl_record(&self) -> fid_rdl::GeneratorRecord {
        fid_rdl::GeneratorRecord {
            id: format!("{}:{}", self.family, self.method),
            version: self.tool_version.clone(),
            seed: self.seed.to_string(),
        }
    }
}

/// Creates the deterministic counter-based geometry RNG stream for `(seed, family_ordinal)`
/// using the kernel Philox4x32-10 RNG (`KRN.NUM.RNG`).
pub fn geometry_rng(seed: u64, family_ordinal: u64) -> Cursor {
    Cursor::new(Address { seed, stream: family_ordinal }, 0)
}

/// Draws a deterministic uniform value in `[0, 1)` as exact [`Hp`] from `cursor`
/// (`u64` top 53 bits scaled by `2^-53`, identical bits on x86-64, ARM64 and wasm32).
pub fn hp_uniform(cursor: &mut Cursor) -> Hp {
    let bits = cursor.next_u64() >> 11;
    Hp::from_u64(bits).scale_pow2(-53)
}
