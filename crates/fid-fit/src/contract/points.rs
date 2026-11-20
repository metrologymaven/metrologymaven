//! The borrowed point view (REQ-KRN.LSQ.TRAIT-002).

use fid_num::red;

use super::error::FitError;

/// Largest number of points a [`PointsView`] may hold, so every pass over it is bounded.
pub const MAX_POINTS: usize = 1 << 24;

/// Largest accepted `| ||n|| - 1 |` of a normal, with `||n||` from `red::norm2`.
pub const UNIT_NORMAL_TOL: f64 = 1e-6;

/// A borrowed, validated view over contiguous `f64` xyz triples in SI metres, with optional
/// unit normals in the same layout. The fields are private so every value went through
/// [`PointsView::new`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointsView<'a> {
    xyz: &'a [f64],
    normals: Option<&'a [f64]>,
}

impl<'a> PointsView<'a> {
    /// Validates and wraps the buffers. Check order (each scan in ascending element order):
    ///
    /// 1. `xyz.len()` not a multiple of 3: [`FitError::BadLength`];
    /// 2. more than [`MAX_POINTS`] points: [`FitError::TooLarge`];
    /// 3. `normals.len() != xyz.len()`: [`FitError::LengthMismatch`];
    /// 4. the first coordinate that is NaN or infinite ([`FitError::NonFinite`]) or a non-zero
    ///    subnormal ([`FitError::Subnormal`]), reported with its point index;
    /// 5. the first normal with a NaN or infinite component ([`FitError::NonFiniteNormal`]),
    ///    then (in a second pass) the first normal whose `red::norm2` differs from 1 by more
    ///    than [`UNIT_NORMAL_TOL`] ([`FitError::NonUnitNormal`]).
    ///
    /// An empty view is valid; fits on it fail with [`FitError::TooFewPoints`].
    pub fn new(xyz: &'a [f64], normals: Option<&'a [f64]>) -> Result<Self, FitError> {
        if xyz.len() % 3 != 0 {
            return Err(FitError::BadLength { len: xyz.len() });
        }
        let n = xyz.len() / 3;
        if n > MAX_POINTS {
            return Err(FitError::TooLarge { n, max: MAX_POINTS });
        }
        if let Some(nm) = normals {
            if nm.len() != xyz.len() {
                return Err(FitError::LengthMismatch { points: xyz.len(), normals: nm.len() });
            }
        }
        for (i, &x) in xyz.iter().enumerate() {
            if !x.is_finite() {
                return Err(FitError::NonFinite { index: i / 3 });
            }
            if x.is_subnormal() {
                return Err(FitError::Subnormal { index: i / 3 });
            }
        }
        if let Some(nm) = normals {
            for (i, x) in nm.iter().enumerate() {
                if !x.is_finite() {
                    return Err(FitError::NonFiniteNormal { index: i / 3 });
                }
            }
            for (i, c) in nm.chunks_exact(3).enumerate() {
                let len = red::norm2(c).map_err(|_| FitError::NonUnitNormal { index: i })?;
                let dev = len - 1.0;
                if dev.abs() > UNIT_NORMAL_TOL {
                    return Err(FitError::NonUnitNormal { index: i });
                }
            }
        }
        Ok(PointsView { xyz, normals })
    }

    /// Number of points.
    pub fn len(&self) -> usize {
        self.xyz.len() / 3
    }

    /// Whether the view holds no point.
    pub fn is_empty(&self) -> bool {
        self.xyz.is_empty()
    }

    /// The coordinate buffer (`3 * len()` values, point `i` at `3i .. 3i + 3`).
    pub fn xyz(&self) -> &'a [f64] {
        self.xyz
    }

    /// The normal buffer, if any (same layout as [`PointsView::xyz`]).
    pub fn normals(&self) -> Option<&'a [f64]> {
        self.normals
    }

    /// Point `i`, or `None` when out of range.
    pub fn point(&self, i: usize) -> Option<[f64; 3]> {
        triple(self.xyz, i)
    }

    /// The normal of point `i`, or `None` when out of range or when the view has no normals.
    pub fn normal(&self, i: usize) -> Option<[f64; 3]> {
        self.normals.and_then(|nm| triple(nm, i))
    }
}

fn triple(buf: &[f64], i: usize) -> Option<[f64; 3]> {
    let s = buf.get(i.checked_mul(3)?..i.checked_mul(3)?.checked_add(3)?)?;
    Some([s[0], s[1], s[2]])
}
