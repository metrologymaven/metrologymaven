// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use crate::algebra::T;
use crate::algebra::{nat_log, FloatT, ScalarMath};

impl ScalarMath for T {
    fn clip(&self, min_thresh: Self, max_thresh: Self) -> Self {
        if *self < min_thresh {
            min_thresh
        } else if *self > max_thresh {
            max_thresh
        } else {
            *self
        }
    }

    fn logsafe(&self) -> Self {
        if *self <= Self::zero() {
            -Self::infinity()
        } else {
            nat_log(*self)
        }
    }
}

/// k(k+1)/2, or `None` on overflow (patch 0003: checked instead of panicking)
pub(crate) const fn triangular_number(k: usize) -> Option<usize> {
    match k.checked_add(1) {
        Some(k1) => match k.checked_mul(k1) {
            Some(p) => Some(p >> 1),
            None => None,
        },
        None => None,
    }
}

#[allow(dead_code)]
pub(crate) fn triangular_index(k: usize) -> usize {
    // 0-based index into a packed triangle. Same as:
    // triangular number(k+1) - 1 = (((k+1) * (k+2)) >> 1) - 1
    (k * (k + 3)) >> 1
}
