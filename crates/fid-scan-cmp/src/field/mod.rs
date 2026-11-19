//! The `DeviationField` element, its crate-private builder and its canonical digest (spec
//! node `SCAN.CMP.DEV.FIELD`).
//!
//! A field stores one status and one value per actual point, in original point order, plus
//! the [`FieldProvenance`] that makes the comparison reproducible. Only [`FieldBuilder`],
//! which is `pub(crate)` and is handed out by the CPU deviation path, creates a field
//! (REQ-SCAN.CMP.DEV.FIELD-002, REQ-SCAN.CMP.DEV-005); see the crate documentation for the
//! compile-fail checks.
//!
//! # Values of record
//!
//! - A `VALID` value is a finite `f64` in metres, signed positive on the material-outside
//!   side of the nominal (the sign comes from the primitive, SCAN.CMP.DEV.PRIM). NaN or an
//!   infinity is refused with `scan.cmp.non_finite` when it is set.
//! - A `VALID` value of `-0.0` is stored as `+0.0`; every other value keeps its bits.
//! - A point that is not `VALID` stores `+0.0` and is left out of every statistic
//!   (REQ-SCAN.CMP.DEV.FIELD-004).
//!
//! # Layout version 1 and digest (REQ-SCAN.CMP.DEV.FIELD-005)
//!
//! All integers little-endian:
//!
//! ```text
//! "FIDDEV" | 0x01 | prov_len u64 | provenance canonical JSON (prov_len bytes)
//! | n u64 | n x (status u8 | value f64 IEEE 754 bits)
//! ```
//!
//! Status bytes: `VALID` 1, `NO_MATCH` 2, `EXCLUDED` 3. The digest is SHA-256 (`fid-kci`'s
//! portable FIPS 180-4 code) over the layout, so it depends only on the values in original
//! index order, never on fill order, chunking or thread count. Only integer and byte
//! operations are used (D0).
//!
//! # Provenance canonical JSON
//!
//! `fid-kci`'s kernel canonical JSON (RFC 8785 restricted to strings, arrays and objects;
//! every number is a string). Members, in canonical order: `actual_digest`,
//! `actual_selection` (omitted when absent), `algorithm_id`, `alignment_digest`,
//! `alignment_id`, `kernel_version`, `max_angle_rad`, `max_distance_m`, `method`,
//! `nominal_digest`, `nominal_kind` (the GEO.TYPES.ENV wire name), `nominal_selection`
//! (omitted when absent). Digests are in the `sha256:` wire form; the two numbers are the
//! ECMAScript `Number::toString` of the `f64` (`-0.0` gives `"0"`).
//!
//! The golden vectors in `fixtures/scan/cmp/field_golden.json` come from the independent
//! Python reference `fixtures/scan/cmp/make_field_golden.py`.

use crate::dev::{DevResult, Method};
use crate::error::{FieldError, IncompleteKind};
use crate::number::es_number;
use fid_geo_types::ElementKind;
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::Sha256;
use fid_kci::AlgorithmId;
use fid_types::Digest;

#[cfg(test)]
mod tests;

/// The element kind name a deviation field is registered under.
pub const ELEMENT_KIND: &str = "deviation_field";

/// The ASCII tag every layout starts with.
pub const LAYOUT_TAG: [u8; 6] = *b"FIDDEV";

/// The layout version this module writes.
pub const LAYOUT_VERSION: u8 = 1;

/// Points per block when the per-point part of the layout is streamed to the hasher.
const POINTS_PER_BLOCK: usize = 1024;

/// Bytes per point in the layout: status byte and value.
const POINT_BYTES: usize = 9;

/// The status of one point (REQ-SCAN.CMP.DEV.FIELD-001). The discriminant is the layout's
/// status byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum DevStatus {
    /// The point has a value of record.
    Valid = 1,
    /// No nominal match (beyond the search distance, angle cutoff or off edge).
    NoMatch = 2,
    /// The point was not compared.
    Excluded = 3,
}

impl DevStatus {
    /// Every status, in byte order.
    pub const ALL: [DevStatus; 3] = [DevStatus::Valid, DevStatus::NoMatch, DevStatus::Excluded];

    /// The layout byte.
    pub const fn byte(self) -> u8 {
        self as u8
    }

    /// The wire name: `VALID`, `NO_MATCH` or `EXCLUDED`.
    pub fn as_str(self) -> &'static str {
        match self {
            DevStatus::Valid => "VALID",
            DevStatus::NoMatch => "NO_MATCH",
            DevStatus::Excluded => "EXCLUDED",
        }
    }
}

/// Everything that makes a comparison reproducible (REQ-SCAN.CMP.DEV.FIELD-001,
/// REQ-SCAN.CMP.DEV-003). Lengths in metres, angles in radians.
#[derive(Debug, Clone, PartialEq)]
pub struct FieldProvenance {
    pub method: Method,
    /// Maximum search distance in metres; finite and `> 0`.
    pub max_distance_m: f64,
    /// Normal-angle cutoff in radians; in `[0, PI]` (`core::f64::consts::PI`).
    pub max_angle_rad: f64,
    /// The alignment the comparison ran in; not empty.
    pub alignment_id: String,
    pub alignment_digest: Digest,
    pub actual_digest: Digest,
    pub nominal_digest: Digest,
    pub nominal_kind: ElementKind,
    pub actual_selection: Option<Digest>,
    pub nominal_selection: Option<Digest>,
    pub algorithm_id: AlgorithmId,
    /// The kernel version string; not empty.
    pub kernel_version: String,
}

impl FieldProvenance {
    /// Checks the parameter domains (`scan.cmp.invalid_parameter`).
    pub fn validate(&self) -> Result<(), FieldError> {
        let bad = |name| Err(FieldError::InvalidParameter { name });
        if !(self.max_distance_m.is_finite() && self.max_distance_m > 0.0) {
            return bad("max_distance_m");
        }
        if !(0.0..=core::f64::consts::PI).contains(&self.max_angle_rad) {
            return bad("max_angle_rad");
        }
        if self.alignment_id.is_empty() {
            return bad("alignment_id");
        }
        if self.kernel_version.is_empty() {
            return bad("kernel_version");
        }
        Ok(())
    }

    /// The canonical JSON bytes of the provenance (see the module documentation).
    pub fn canonical_json(&self) -> Result<Vec<u8>, FieldError> {
        self.validate()?;
        let num = |name, v| es_number(v).map(Json::Str).ok_or(FieldError::InvalidParameter { name });
        let digest = |d: &Digest| Json::Str(d.as_str().to_string());
        let mut members = vec![
            ("actual_digest".to_string(), digest(&self.actual_digest)),
            ("algorithm_id".to_string(), Json::Str(self.algorithm_id.to_string())),
            ("alignment_digest".to_string(), digest(&self.alignment_digest)),
            ("alignment_id".to_string(), Json::Str(self.alignment_id.clone())),
            ("kernel_version".to_string(), Json::Str(self.kernel_version.clone())),
            ("max_angle_rad".to_string(), num("max_angle_rad", self.max_angle_rad)?),
            ("max_distance_m".to_string(), num("max_distance_m", self.max_distance_m)?),
            ("method".to_string(), Json::Str(self.method.as_str().to_string())),
            ("nominal_digest".to_string(), digest(&self.nominal_digest)),
            ("nominal_kind".to_string(), Json::Str(self.nominal_kind.as_str().to_string())),
        ];
        if let Some(d) = &self.actual_selection {
            members.push(("actual_selection".to_string(), digest(d)));
        }
        if let Some(d) = &self.nominal_selection {
            members.push(("nominal_selection".to_string(), digest(d)));
        }
        // The keys are fixed and unique and the depth is 1, so the writer cannot fail.
        to_canonical(&Json::Obj(members)).map_err(|_| FieldError::InvalidParameter { name: "provenance" })
    }
}

/// Counts per status and, over the `VALID` values only, the totalOrder minimum and
/// maximum and the fixed-order mean (`fid-num` KRN.NUM.RED). The three values are `None`
/// when no point is `VALID`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldStats {
    pub valid: u64,
    pub no_match: u64,
    pub excluded: u64,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub mean: Option<f64>,
}

/// A stored comparison: one status and one value per actual point, provenance and digest.
/// No public constructor; built only by the crate's CPU kernel path.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviationField {
    values: Vec<f64>,
    status: Vec<DevStatus>,
    provenance: FieldProvenance,
    provenance_json: Vec<u8>,
    digest: Digest,
}

impl DeviationField {
    /// The number of actual points.
    pub fn len(&self) -> usize {
        self.status.len()
    }

    /// Whether the field has no points.
    pub fn is_empty(&self) -> bool {
        self.status.is_empty()
    }

    /// The value of point `i` if it is `VALID`; `None` otherwise or out of range.
    pub fn value(&self, i: usize) -> Option<f64> {
        match self.status.get(i) {
            Some(DevStatus::Valid) => self.values.get(i).copied(),
            _ => None,
        }
    }

    /// The status of point `i`; `None` out of range.
    pub fn status(&self, i: usize) -> Option<DevStatus> {
        self.status.get(i).copied()
    }

    /// The value column (`+0.0` where the point is not `VALID`).
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    /// The status column.
    pub fn statuses(&self) -> &[DevStatus] {
        &self.status
    }

    /// The provenance.
    pub fn provenance(&self) -> &FieldProvenance {
        &self.provenance
    }

    /// The canonical JSON of the provenance, as hashed.
    pub fn provenance_json(&self) -> &[u8] {
        &self.provenance_json
    }

    /// The SHA-256 digest of layout version 1.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// The indexes of the `VALID` points, ascending.
    pub fn valid_indexes(&self) -> impl Iterator<Item = usize> + '_ {
        self.status.iter().enumerate().filter(|(_, s)| **s == DevStatus::Valid).map(|(i, _)| i)
    }

    /// Statistics over the `VALID` points only. Fails `scan.cmp.non_finite` if the sum of
    /// the values overflows, and `scan.cmp.allocation_failed` if the value copy the
    /// fixed-order mean needs cannot be allocated.
    pub fn stats(&self) -> Result<FieldStats, FieldError> {
        let mut st = FieldStats { valid: 0, no_match: 0, excluded: 0, min: None, max: None, mean: None };
        for s in &self.status {
            match s {
                DevStatus::Valid => st.valid += 1,
                DevStatus::NoMatch => st.no_match += 1,
                DevStatus::Excluded => st.excluded += 1,
            }
        }
        if st.valid == 0 {
            return Ok(st);
        }
        let mut vals: Vec<f64> = Vec::new();
        vals.try_reserve_exact(st.valid as usize)
            .map_err(|_| FieldError::AllocationFailed { needed: st.valid.saturating_mul(8) })?;
        vals.extend(self.valid_indexes().filter_map(|i| self.values.get(i).copied()));
        let overflow = |_| FieldError::NonFinite { index: None };
        let Some((&first, rest)) = vals.split_first() else {
            return Ok(st);
        };
        let (mut lo, mut hi) = (first, first);
        for &v in rest {
            lo = fid_num::f::total_min(lo, v).map_err(overflow)?;
            hi = fid_num::f::total_max(hi, v).map_err(overflow)?;
        }
        st.min = Some(lo);
        st.max = Some(hi);
        st.mean = Some(fid_num::red::mean(&vals).map_err(overflow)?);
        Ok(st)
    }

    /// Streams layout version 1 to `sink` in blocks.
    pub fn write_layout(&self, sink: impl FnMut(&[u8])) {
        write_layout(&self.provenance_json, &self.status, &self.values, sink);
    }
}

fn write_layout(prov: &[u8], status: &[DevStatus], values: &[f64], mut sink: impl FnMut(&[u8])) {
    let mut head = Vec::with_capacity(LAYOUT_TAG.len() + 1 + 8);
    head.extend_from_slice(&LAYOUT_TAG);
    head.push(LAYOUT_VERSION);
    head.extend_from_slice(&(prov.len() as u64).to_le_bytes());
    sink(&head);
    sink(prov);
    sink(&(status.len() as u64).to_le_bytes());
    let mut block = [0u8; POINT_BYTES * POINTS_PER_BLOCK];
    for (s_chunk, v_chunk) in status.chunks(POINTS_PER_BLOCK).zip(values.chunks(POINTS_PER_BLOCK)) {
        let mut used = 0;
        for (s, v) in s_chunk.iter().zip(v_chunk) {
            let rec = &mut block[used..used + POINT_BYTES];
            rec[0] = s.byte();
            rec[1..].copy_from_slice(&v.to_bits().to_le_bytes());
            used += POINT_BYTES;
        }
        sink(&block[..used]);
    }
}

#[cfg_attr(not(test), allow(dead_code))]
fn layout_digest(prov: &[u8], status: &[DevStatus], values: &[f64]) -> Digest {
    let mut h = Sha256::new();
    write_layout(prov, status, values, |b| h.update(b));
    Digest::from_bytes(h.finish())
}

/// The only way to create a [`DeviationField`]: one `set` per point index in any order,
/// then `finish`. Crate-private; the CPU deviation path (SCAN.CMP.DEV.PRIM's
/// `deviation_batch`, not yet built) hands it out.
#[derive(Debug)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) struct FieldBuilder {
    provenance: FieldProvenance,
    provenance_json: Vec<u8>,
    values: Vec<f64>,
    status: Vec<DevStatus>,
    /// Times each index was set: 0, 1, or 2 for "more than once".
    seen: Vec<u8>,
}

#[cfg_attr(not(test), allow(dead_code))]
impl FieldBuilder {
    /// A builder for `n` points. Fails `scan.cmp.invalid_parameter` on a bad provenance
    /// and `scan.cmp.allocation_failed` if the three columns (10 bytes per point) cannot be
    /// allocated.
    pub(crate) fn new(n: usize, provenance: FieldProvenance) -> Result<FieldBuilder, FieldError> {
        let provenance_json = provenance.canonical_json()?;
        let needed = (n as u64).saturating_mul(10);
        let fail = |_| FieldError::AllocationFailed { needed };
        let mut values: Vec<f64> = Vec::new();
        values.try_reserve_exact(n).map_err(fail)?;
        let mut status: Vec<DevStatus> = Vec::new();
        status.try_reserve_exact(n).map_err(fail)?;
        let mut seen: Vec<u8> = Vec::new();
        seen.try_reserve_exact(n).map_err(fail)?;
        values.resize(n, 0.0);
        status.resize(n, DevStatus::NoMatch);
        seen.resize(n, 0);
        Ok(FieldBuilder { provenance, provenance_json, values, status, seen })
    }

    /// Records the result of point `index`. Fails `scan.cmp.field_index_out_of_range` for
    /// an index not below the count (checked first, whatever the result), then
    /// `scan.cmp.non_finite` for a NaN or infinite `VALID` value; in both cases nothing is
    /// recorded. Setting an index twice is
    /// recorded and reported by [`FieldBuilder::finish`].
    pub(crate) fn set(&mut self, index: usize, result: DevResult) -> Result<(), FieldError> {
        let len = self.seen.len();
        if index >= len {
            return Err(FieldError::IndexOutOfRange { index: index as u64, len: len as u64 });
        }
        let (status, value) = match result {
            DevResult::Valid(v) if !v.is_finite() => return Err(FieldError::NonFinite { index: Some(index as u64) }),
            // `v == 0.0` holds for both zeros: -0.0 is stored as +0.0.
            DevResult::Valid(v) => (DevStatus::Valid, if v == 0.0 { 0.0 } else { v }),
            DevResult::NoMatch(_) => (DevStatus::NoMatch, 0.0),
            DevResult::Excluded => (DevStatus::Excluded, 0.0),
        };
        if let (Some(seen), Some(s), Some(v)) =
            (self.seen.get_mut(index), self.status.get_mut(index), self.values.get_mut(index))
        {
            *seen = if *seen == 0 { 1 } else { 2 };
            *s = status;
            *v = value;
        }
        Ok(())
    }

    /// The finished field. Fails `scan.cmp.field_incomplete` naming the lowest index that
    /// is unset or was set more than once.
    pub(crate) fn finish(self) -> Result<DeviationField, FieldError> {
        if let Some(i) = self.seen.iter().position(|&n| n != 1) {
            let kind = if self.seen[i] == 0 { IncompleteKind::Unset } else { IncompleteKind::SetTwice };
            return Err(FieldError::Incomplete { index: i as u64, kind });
        }
        let digest = layout_digest(&self.provenance_json, &self.status, &self.values);
        Ok(DeviationField {
            values: self.values,
            status: self.status,
            provenance: self.provenance,
            provenance_json: self.provenance_json,
            digest,
        })
    }
}
