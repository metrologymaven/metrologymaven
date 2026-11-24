//! The exclusion mask and its digest (REQ-KRN.OUT.POL-002, -005; REQ-KRN.OUT-002).

use fid_fit::contract::MAX_POINTS;
use fid_kci::kci::sha256::Sha256;
use fid_types::Digest;

use super::error::{alloc, invalid, PolicyError};

/// Indices hashed per update of the digest.
const DIGEST_CHUNK: usize = 1024;

/// The set of excluded point indices of one input set: sorted ascending, unique, every index
/// below [`point_count`](ExclusionMask::point_count). The fields are private, so every value
/// went through [`ExclusionMask::new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExclusionMask {
    point_count: u32,
    indices: Vec<u32>,
}

impl ExclusionMask {
    /// Builds the mask of `indices` over an input of `point_count` points.
    ///
    /// 1. `point_count` above `fid_fit::contract::MAX_POINTS`:
    ///    `InvalidParameter { field: "point_count", reason: "out_of_range" }`;
    /// 2. the first index, in the given order, that is not below `point_count`:
    ///    [`PolicyError::IndexOutOfRange`] (REQ-KRN.OUT.POL-005);
    /// 3. the indices are sorted ascending; if one is named more than once,
    ///    [`PolicyError::DuplicateIndex`] with the smallest repeated index
    ///    (REQ-KRN.OUT.POL-002: a mask holds unique indices; a repeat means the method
    ///    miscounted, so it is refused, not merged).
    pub fn new(point_count: usize, mut indices: Vec<u32>) -> Result<ExclusionMask, PolicyError> {
        if point_count > MAX_POINTS {
            return Err(invalid("point_count", "out_of_range"));
        }
        let n = u32::try_from(point_count).map_err(|_| invalid("point_count", "out_of_range"))?;
        if let Some(&index) = indices.iter().find(|&&i| i >= n) {
            return Err(PolicyError::IndexOutOfRange { index });
        }
        indices.sort_unstable();
        if let Some(w) = indices.windows(2).find(|w| w[0] == w[1]) {
            return Err(PolicyError::DuplicateIndex { index: w[0] });
        }
        Ok(ExclusionMask { point_count: n, indices })
    }

    /// The mask that excludes nothing.
    pub fn none(point_count: usize) -> Result<ExclusionMask, PolicyError> {
        ExclusionMask::new(point_count, Vec::new())
    }

    /// Number of points of the input set the mask refers to.
    pub fn point_count(&self) -> usize {
        self.point_count as usize
    }

    /// The excluded indices, ascending and unique.
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Number of excluded points.
    pub fn len(&self) -> usize {
        self.indices.len()
    }

    /// Whether no point is excluded.
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    /// Number of points the mask keeps.
    pub fn remaining(&self) -> usize {
        self.point_count().saturating_sub(self.len())
    }

    /// Whether point `i` is excluded.
    pub fn contains(&self, i: u32) -> bool {
        self.indices.binary_search(&i).is_ok()
    }

    /// The canonical bytes the digest is computed over: the point count, then each excluded
    /// index in ascending order, all as little-endian `u32` (the layout of
    /// REQ-KRN.OUT.DISC-001).
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PolicyError> {
        let len = self.indices.len().checked_add(1).and_then(|n| n.checked_mul(4)).ok_or(alloc("mask.bytes"))?;
        let mut out = Vec::new();
        out.try_reserve_exact(len).map_err(|_| alloc("mask.bytes"))?;
        out.extend_from_slice(&self.point_count.to_le_bytes());
        for i in &self.indices {
            out.extend_from_slice(&i.to_le_bytes());
        }
        Ok(out)
    }

    /// SHA-256 of [`canonical_bytes`](ExclusionMask::canonical_bytes), computed incrementally
    /// so a large mask needs no second buffer. An empty mask has a digest too: "nothing was
    /// excluded" is recorded, not left implicit.
    pub fn digest(&self) -> Digest {
        let mut h = Sha256::new();
        h.update(&self.point_count.to_le_bytes());
        let mut buf = [0u8; 4 * DIGEST_CHUNK];
        for chunk in self.indices.chunks(DIGEST_CHUNK) {
            for (slot, i) in buf.chunks_exact_mut(4).zip(chunk) {
                slot.copy_from_slice(&i.to_le_bytes());
            }
            h.update(&buf[..4 * chunk.len()]);
        }
        Digest::from_bytes(h.finish())
    }
}
