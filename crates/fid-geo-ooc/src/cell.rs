//! Octree cells, the point-to-cell rule and Morton codes (pinned; see the crate docs).

use crate::error::OocError;
use fid_geo_types::Aabb;

/// Deepest octree level. Cells at this depth are never split.
pub const MAX_DEPTH: u8 = 21;
/// Cells per axis at [`MAX_DEPTH`]: 2^21.
pub const GRID_CELLS: u64 = 1 << 21;
/// Largest quantised coordinate.
const QMAX: u64 = GRID_CELLS - 1;
/// [`GRID_CELLS`] as `f64` (exact).
const GRID_F64: f64 = 2_097_152.0;

/// An octree cell: depth and Morton prefix in one `u64` with a sentinel bit,
/// `(1 << 3 * depth) | prefix` where `prefix < 8^depth`. The root is `1`; child `o`
/// (octant bits: 1 = upper x half, 2 = upper y half, 4 = upper z half) of `c` is
/// `c << 3 | o`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CellCode(u64);

impl CellCode {
    /// The root cell (depth 0), the whole bounding box.
    pub const ROOT: CellCode = CellCode(1);

    /// The cell at `depth` with Morton `prefix`, if `depth <= 21` and `prefix < 8^depth`.
    pub fn new(depth: u8, prefix: u64) -> Option<CellCode> {
        if depth > MAX_DEPTH {
            return None;
        }
        let shift = 3 * u32::from(depth);
        if prefix >> shift != 0 {
            return None;
        }
        Some(CellCode((1u64 << shift) | prefix))
    }

    /// A cell from its raw code, if the code is valid: non-zero, highest set bit at a
    /// multiple of three, depth at most 21.
    pub fn from_raw(raw: u64) -> Option<CellCode> {
        if raw == 0 {
            return None;
        }
        let top = 63 - raw.leading_zeros();
        if top % 3 != 0 || top / 3 > u32::from(MAX_DEPTH) {
            return None;
        }
        Some(CellCode(raw))
    }

    /// The raw code.
    pub const fn raw(self) -> u64 {
        self.0
    }

    /// The depth, 0 (root) to 21.
    pub fn depth(self) -> u8 {
        ((63 - self.0.leading_zeros()) / 3) as u8
    }

    /// The Morton prefix (the top `3 * depth` bits of the Morton code of every point in
    /// the cell).
    pub fn prefix(self) -> u64 {
        self.0 ^ (1u64 << (3 * u32::from(self.depth())))
    }

    /// Child `octant` (0..8), if this cell is above the depth limit.
    pub fn child(self, octant: u8) -> Option<CellCode> {
        if self.depth() >= MAX_DEPTH || octant > 7 {
            return None;
        }
        Some(CellCode((self.0 << 3) | u64::from(octant)))
    }

    /// Bits of a full Morton code below this cell's prefix.
    fn low_bits(self) -> u32 {
        3 * u32::from(MAX_DEPTH - self.depth())
    }

    /// The first full Morton code in the cell; leaves sort by it (Morton order).
    pub fn range_start(self) -> u64 {
        self.prefix() << self.low_bits()
    }

    /// One past the last full Morton code in the cell (at most 2^63).
    pub fn range_end(self) -> u64 {
        self.range_start() + (1u64 << self.low_bits())
    }

    /// Whether a point with full Morton code `m` lies in this cell.
    pub fn contains(self, m: u64) -> bool {
        m >> self.low_bits() == self.prefix()
    }
}

/// The point-to-cell rule of a partition: per-axis quantisation of the bounding box into
/// 2^21 cells and the 63-bit Morton code.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    lo: [f64; 3],
    hi: [f64; 3],
    ext: [f64; 3],
}

impl Grid {
    /// The grid of `bounds`. Fails if an extent `max - min` overflows to infinity.
    pub fn new(bounds: &Aabb) -> Result<Grid, OocError> {
        let (lo, hi) = (bounds.min(), bounds.max());
        let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        if let Some(axis) = ext.iter().position(|e| !e.is_finite()) {
            return Err(OocError::BoundsTooLarge { axis: axis as u8 });
        }
        Ok(Grid { lo, hi, ext })
    }

    /// Whether `p` lies inside the bounds (numeric comparison, so `-0.0 == +0.0`).
    pub fn inside(&self, p: [f64; 3]) -> bool {
        (0..3).all(|k| self.lo[k] <= p[k] && p[k] <= self.hi[k])
    }

    /// The quantised coordinate on axis `k`: `s = ((v - min) / (max - min)) * 2^21`, then
    /// 0 if `!(s > 0)` (also for a zero extent), else `min(floor(s), 2^21 - 1)`. Each step
    /// is one IEEE 754 operation (round to nearest even, no fused multiply-add), so the
    /// result is the same on every platform. Cell intervals are half-open
    /// `[lower, upper)`, except that the last cell on each axis also holds the maximum.
    pub fn quantise(&self, v: f64, k: usize) -> u64 {
        let (lo, ext) = match (self.lo.get(k), self.ext.get(k)) {
            (Some(&lo), Some(&ext)) => (lo, ext),
            _ => return 0,
        };
        if ext.is_nan() || ext <= 0.0 {
            return 0;
        }
        let s = ((v - lo) / ext) * GRID_F64;
        if s.is_nan() || s <= 0.0 {
            return 0;
        }
        if s >= QMAX as f64 {
            QMAX
        } else {
            s as u64
        }
    }

    /// The Morton code of `p`: bit `3b + k` is bit `b` of the quantised coordinate on axis
    /// `k` (x = 0, y = 1, z = 2).
    pub fn morton(&self, p: [f64; 3]) -> u64 {
        spread(self.quantise(p[0], 0)) | (spread(self.quantise(p[1], 1)) << 1) | (spread(self.quantise(p[2], 2)) << 2)
    }
}

/// Spreads the low 21 bits of `v` to every third bit.
fn spread(v: u64) -> u64 {
    let mut x = v & QMAX;
    x = (x | (x << 32)) & 0x001f_0000_0000_ffff;
    x = (x | (x << 16)) & 0x001f_0000_ff00_00ff;
    x = (x | (x << 8)) & 0x100f_00f0_0f00_f00f;
    x = (x | (x << 4)) & 0x10c3_0c30_c30c_30c3;
    x = (x | (x << 2)) & 0x1249_2492_4924_9249;
    x
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    fn naive(v: u64) -> u64 {
        (0..21).fold(0, |acc, b| acc | (((v >> b) & 1) << (3 * b)))
    }

    #[test]
    fn spread_matches_naive() {
        for v in [0, 1, 2, 3, 0x15_5555, 0x0a_aaaa, QMAX, 12345, 1 << 20, 0x1f_ffff ^ 0x40] {
            assert_eq!(spread(v), naive(v), "{v:#x}");
        }
        let mut v = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..1000 {
            v = v.rotate_left(7) ^ 0x2545_f491_4f6c_dd1d;
            assert_eq!(spread(v & QMAX), naive(v & QMAX));
        }
    }

    #[test]
    fn cell_codes() {
        assert_eq!(CellCode::ROOT.depth(), 0);
        assert_eq!(CellCode::ROOT.prefix(), 0);
        assert_eq!(CellCode::ROOT.range_start(), 0);
        assert_eq!(CellCode::ROOT.range_end(), 1 << 63);
        let c = CellCode::ROOT.child(5).unwrap().child(2).unwrap();
        assert_eq!(c.raw(), 0b1_101_010);
        assert_eq!((c.depth(), c.prefix()), (2, 0b101_010));
        assert_eq!(CellCode::new(2, 0b101_010), Some(c));
        assert_eq!(CellCode::from_raw(c.raw()), Some(c));
        for bad in [0u64, 2, 4, 0b10_000, u64::MAX >> 1] {
            assert_eq!(CellCode::from_raw(bad), None, "{bad:#b}");
        }
        let deepest = CellCode::new(21, (1 << 63) - 1).unwrap();
        assert_eq!(deepest.raw(), u64::MAX);
        assert_eq!(deepest.child(0), None);
        assert_eq!(CellCode::new(22, 0), None);
        assert_eq!(CellCode::new(1, 8), None);
        assert!(c.contains(c.range_start()));
        assert!(c.contains(c.range_end() - 1));
        assert!(!c.contains(c.range_end()));
    }

    #[test]
    fn quantise_boundaries() {
        let b = Aabb::new([0.0, -1.0, 5.0], [1.0, 1.0, 5.0]).unwrap();
        let g = Grid::new(&b).unwrap();
        assert_eq!(g.quantise(0.0, 0), 0);
        assert_eq!(g.quantise(-0.0, 0), 0);
        assert_eq!(g.quantise(0.5, 0), 1 << 20);
        assert_eq!(g.quantise(1.0, 0), QMAX);
        assert_eq!(g.quantise(0.0, 1), 1 << 20);
        assert_eq!(g.quantise(-0.0, 1), 1 << 20);
        assert_eq!(g.quantise(5.0, 2), 0, "zero extent");
        assert!(Grid::new(&Aabb::new([-f64::MAX, 0.0, 0.0], [f64::MAX, 0.0, 0.0]).unwrap()).is_err());
        assert_eq!(g.morton([1.0, 1.0, 5.0]), spread(QMAX) | (spread(QMAX) << 1));
    }
}
