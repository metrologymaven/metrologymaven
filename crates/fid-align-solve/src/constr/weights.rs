//! The weight model: per-point and per-region weights and their combination (module docs of
//! [`crate::constr`]).

use fid_kci::kci::Sha256;
use fid_types::Digest;
use std::fmt;

use super::ConstrError;

/// Most points one constraint set covers.
pub const MAX_POINTS: usize = 1 << 26;
/// Most weighted regions.
pub const MAX_REGIONS: usize = 4096;

/// Bytes buffered before the weight bytes are passed to the hasher.
const SINK_BYTES: usize = 64 * 1024;

/// A region resolved to its points (selector resolution is `ALN.BFIT.LOCAL` / `GEO.SEL`) and
/// its weight.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct RegionWeight {
    /// Point indices, strictly increasing, each below the point count.
    pub members: Vec<u32>,
    /// The weight: `0` or a finite, normal, positive binary64.
    pub weight: f64,
}

/// Per-point weights and per-region weights. The default weighs every point 1.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct WeightSpec {
    /// One weight per point, or `None` for 1 each.
    pub per_point: Option<Vec<f64>>,
    /// Region weights, applied in this order.
    pub per_region: Vec<RegionWeight>,
}

/// The first offending weight input of an `invalid_weight` refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeightInput {
    /// `per_point` does not hold one weight per point.
    PerPointLength { expected: usize, got: usize },
    /// The per-point weight of this point.
    PerPoint { index: usize },
    /// The weight of this region.
    Region { region: usize },
    /// The combined weight of this point would be infinite, subnormal or a spurious zero.
    Combined { point: usize },
    /// No point has a positive combined weight (also: no points).
    AllZero,
}

impl fmt::Display for WeightInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WeightInput::PerPointLength { expected, got } => write!(f, "per_point_length {got}, expected {expected}"),
            WeightInput::PerPoint { index } => write!(f, "per_point {index}"),
            WeightInput::Region { region } => write!(f, "region {region}"),
            WeightInput::Combined { point } => write!(f, "combined {point}"),
            WeightInput::AllZero => f.write_str("all_zero"),
        }
    }
}

/// `+0`, `-0` or a finite, normal, positive binary64.
fn ok_weight(w: f64) -> bool {
    w.is_finite() && (w == 0.0 || w >= f64::MIN_POSITIVE)
}

fn pos0(w: f64) -> f64 {
    if w == 0.0 {
        0.0
    } else {
        w
    }
}

/// The combined weights and the number of positive ones, in the pinned check order.
pub(crate) fn combine(spec: &WeightSpec, n: usize) -> Result<(Vec<f64>, usize), ConstrError> {
    if n == 0 {
        return Err(ConstrError::InvalidWeight(WeightInput::AllZero));
    }
    if n > MAX_POINTS {
        return Err(ConstrError::InputLimit("points"));
    }
    if spec.per_region.len() > MAX_REGIONS {
        return Err(ConstrError::InputLimit("regions"));
    }
    if let Some(pp) = &spec.per_point {
        if pp.len() != n {
            return Err(ConstrError::InvalidWeight(WeightInput::PerPointLength { expected: n, got: pp.len() }));
        }
        if let Some(index) = pp.iter().position(|w| !ok_weight(*w)) {
            return Err(ConstrError::InvalidWeight(WeightInput::PerPoint { index }));
        }
    }
    for (region, r) in spec.per_region.iter().enumerate() {
        if !ok_weight(r.weight) {
            return Err(ConstrError::InvalidWeight(WeightInput::Region { region }));
        }
        let mut prev: Option<u32> = None;
        for &i in &r.members {
            let in_range = usize::try_from(i).map_or(false, |i| i < n);
            if !in_range || prev.map_or(false, |p| i <= p) {
                return Err(ConstrError::InvalidRegion { region });
            }
            prev = Some(i);
        }
    }
    let mut out = Vec::new();
    out.try_reserve_exact(n).map_err(|_| ConstrError::AllocationFailed)?;
    match &spec.per_point {
        Some(pp) => out.extend(pp.iter().map(|w| pos0(*w))),
        None => out.resize(n, 1.0),
    }
    for r in &spec.per_region {
        let rw = pos0(r.weight);
        for &i in &r.members {
            let point = usize::try_from(i).map_err(|_| ConstrError::Internal("member index"))?;
            let slot = out.get_mut(point).ok_or(ConstrError::Internal("member index"))?;
            let a = *slot;
            let p = a * rw;
            if !ok_weight(p) || (p == 0.0 && a != 0.0 && rw != 0.0) {
                return Err(ConstrError::InvalidWeight(WeightInput::Combined { point }));
            }
            *slot = p;
        }
    }
    let n_used = out.iter().filter(|w| **w > 0.0).count();
    if n_used == 0 {
        return Err(ConstrError::InvalidWeight(WeightInput::AllZero));
    }
    Ok((out, n_used))
}

/// SHA-256 of the weights as little-endian binary64 bytes in index order.
pub(crate) fn digest(ws: &[f64]) -> Digest {
    let mut h = Sha256::new();
    let mut buf = [0u8; SINK_BYTES];
    for chunk in ws.chunks(SINK_BYTES / 8) {
        let mut k = 0;
        for w in chunk {
            buf[k..k + 8].copy_from_slice(&w.to_le_bytes());
            k += 8;
        }
        h.update(&buf[..k]);
    }
    Digest::from_bytes(h.finish())
}
