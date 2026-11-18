//! Point sets, point clouds and per-point attributes (spec node `GEO.TYPES.POINTS`).
//!
//! [`PointBuffer`] holds coordinates as three `f64` columns in metres (structure of
//! arrays); [`AttributeSet`] holds the optional per-point channels scanners and probes
//! produce, the per-scanner origins and an optional organised-grid shape. [`PointSet`]
//! (sparse or probed points) and [`PointCloud`] (dense scans) are both built by
//! `new(frame, buffer, attributes)`, which validates everything once, in a fixed order,
//! and reports the first failure:
//!
//! 1. the y and z columns have as many values as x (`AttributeLength`, names `y`, `z`);
//! 2. every coordinate is finite, in point order (`NonFinite { index }`);
//! 3. every present channel has one value per point, in the field order of
//!    [`AttributeSet`] (`AttributeLength { name, expected, found }`);
//! 4. every normal is finite with `| |n| - 1 | <= 1e-9`, in point order
//!    (`BadNormal { index }`); normals are never normalised;
//! 5. scan origins are finite and in strictly ascending scanner-ID order
//!    (`ScanOrigin { scanner_id }`);
//! 6. a declared grid has `rows * cols == count` (`GridShape { rows, cols, count }`).
//!
//! Counts and indexes are `u64` (REQ-GEO.TYPES.POINTS-001, -007). KNP-1: the normal length
//! uses `fid_num::f::sqrt`, sums have a fixed order and no fused multiply-add, there are
//! no hash containers and NaN is always an error.

use crate::envelope::{Aabb, FrameRef};
use crate::error::GeoTypeError;
use crate::source::InMemorySource;
use core::num::NonZeroUsize;

/// The largest accepted deviation of a normal's length from 1 (REQ-GEO.TYPES.POINTS-004).
pub const NORMAL_TOLERANCE: f64 = 1e-9;

/// Coordinates in metres as three columns (structure of arrays). A buffer is plain data;
/// it is validated when an element is built from it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PointBuffer {
    /// The x coordinates, in point order.
    pub x: Vec<f64>,
    /// The y coordinates, in point order.
    pub y: Vec<f64>,
    /// The z coordinates, in point order.
    pub z: Vec<f64>,
}

fn count_of(n: usize) -> u64 {
    n as u64
}

impl PointBuffer {
    /// A buffer from three columns.
    pub fn new(x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> PointBuffer {
        PointBuffer { x, y, z }
    }

    /// A buffer from `[x, y, z]` points.
    pub fn from_points(points: &[[f64; 3]]) -> PointBuffer {
        PointBuffer {
            x: points.iter().map(|p| p[0]).collect(),
            y: points.iter().map(|p| p[1]).collect(),
            z: points.iter().map(|p| p[2]).collect(),
        }
    }

    /// The number of points (the length of the x column).
    pub fn len(&self) -> u64 {
        count_of(self.x.len())
    }

    /// Whether the buffer has no points.
    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Point `i`, if it exists in all three columns.
    pub fn point(&self, i: u64) -> Option<[f64; 3]> {
        self.get(usize::try_from(i).ok()?)
    }

    pub(crate) fn get(&self, i: usize) -> Option<[f64; 3]> {
        Some([*self.x.get(i)?, *self.y.get(i)?, *self.z.get(i)?])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.x.iter().zip(&self.y).zip(&self.z).map(|((&a, &b), &c)| [a, b, c])
    }

    pub(crate) fn aabb(&self) -> Result<Aabb, GeoTypeError> {
        Aabb::of_points(self.iter())
    }

    /// Equal column lengths, then finite coordinates in point order.
    pub(crate) fn validate(&self) -> Result<(), GeoTypeError> {
        let n = self.x.len();
        check_len("y", count_of(n), self.y.len())?;
        check_len("z", count_of(n), self.z.len())?;
        for (i, p) in self.iter().enumerate() {
            if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) {
                return Err(GeoTypeError::NonFinite { index: count_of(i) });
            }
        }
        Ok(())
    }
}

fn check_len(name: &str, expected: u64, found: usize) -> Result<(), GeoTypeError> {
    let found = count_of(found);
    if found == expected {
        Ok(())
    } else {
        Err(GeoTypeError::AttributeLength { name: name.to_string(), expected, found })
    }
}

/// Whether `n` is finite with `| |n| - 1 | <= NORMAL_TOLERANCE`. The squared length is
/// summed as `(x*x + y*y) + z*z` (no fused multiply-add) and its root comes from
/// `fid_num::f::sqrt`; an overflowing sum is an error there and so a bad normal here.
pub fn is_unit_normal(n: [f64; 3]) -> bool {
    if !(n[0].is_finite() && n[1].is_finite() && n[2].is_finite()) {
        return false;
    }
    let s = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
    match fid_num::f::sqrt(s) {
        Ok(len) => (len - 1.0).abs() <= NORMAL_TOLERANCE,
        Err(_) => false,
    }
}

/// The optional per-point channels, per-scanner origins and organised-grid shape.
///
/// Scanner structure (which scanner took a point, where it stood, the grid it scanned)
/// lives here and not in a separate element kind (REQ-GEO.TYPES.POINTS-006).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AttributeSet {
    /// Unit normals as three columns (x, y, z).
    pub normal: Option<[Vec<f64>; 3]>,
    /// Return intensity.
    pub intensity: Option<Vec<f32>>,
    /// RGB colour.
    pub colour: Option<Vec<[u8; 3]>>,
    /// The scanner (or scan) that produced each point.
    pub scanner_id: Option<Vec<u32>>,
    /// Acquisition time in nanoseconds.
    pub timestamp_ns: Option<Vec<i64>>,
    /// Quality flags.
    pub quality: Option<Vec<u16>>,
    /// Scanner origins in metres, in strictly ascending scanner-ID order (no hash map).
    pub scan_origins: Vec<(u32, [f64; 3])>,
    /// The organised-grid shape `(rows, cols)`, row-major, if the points form one.
    pub grid: Option<(u64, u64)>,
}

impl AttributeSet {
    /// The names of the per-point channels that are present, in field order.
    pub fn channel_names(&self) -> Vec<&'static str> {
        let present = [
            ("normal", self.normal.is_some()),
            ("intensity", self.intensity.is_some()),
            ("colour", self.colour.is_some()),
            ("scanner_id", self.scanner_id.is_some()),
            ("timestamp_ns", self.timestamp_ns.is_some()),
            ("quality", self.quality.is_some()),
        ];
        present.iter().filter(|(_, p)| *p).map(|(n, _)| *n).collect()
    }

    /// The origin of scanner `id`, if one is recorded.
    pub fn scan_origin(&self, id: u32) -> Option<[f64; 3]> {
        self.scan_origins.iter().find(|(s, _)| *s == id).map(|(_, o)| *o)
    }

    /// Normal `i`, if normals are present and `i` exists.
    pub fn normal_at(&self, i: u64) -> Option<[f64; 3]> {
        let i = usize::try_from(i).ok()?;
        let n = self.normal.as_ref()?;
        Some([*n[0].get(i)?, *n[1].get(i)?, *n[2].get(i)?])
    }

    /// Checks 3 to 6 of the module documentation against `count` points.
    fn validate(&self, count: u64) -> Result<(), GeoTypeError> {
        if let Some(n) = &self.normal {
            for col in n {
                check_len("normal", count, col.len())?;
            }
        }
        if let Some(v) = &self.intensity {
            check_len("intensity", count, v.len())?;
        }
        if let Some(v) = &self.colour {
            check_len("colour", count, v.len())?;
        }
        if let Some(v) = &self.scanner_id {
            check_len("scanner_id", count, v.len())?;
        }
        if let Some(v) = &self.timestamp_ns {
            check_len("timestamp_ns", count, v.len())?;
        }
        if let Some(v) = &self.quality {
            check_len("quality", count, v.len())?;
        }
        if let Some([nx, ny, nz]) = &self.normal {
            for (i, ((&a, &b), &c)) in nx.iter().zip(ny).zip(nz).enumerate() {
                if !is_unit_normal([a, b, c]) {
                    return Err(GeoTypeError::BadNormal { index: count_of(i) });
                }
            }
        }
        let mut prev: Option<u32> = None;
        for &(id, o) in &self.scan_origins {
            let ordered = match prev {
                Some(p) => id > p,
                None => true,
            };
            if !(ordered && o[0].is_finite() && o[1].is_finite() && o[2].is_finite()) {
                return Err(GeoTypeError::ScanOrigin { scanner_id: id });
            }
            prev = Some(id);
        }
        if let Some((rows, cols)) = self.grid {
            if rows.checked_mul(cols) != Some(count) {
                return Err(GeoTypeError::GridShape { rows, cols, count });
            }
        }
        Ok(())
    }
}

/// Validates a buffer and its attributes in the documented order.
fn validate(buf: &PointBuffer, attrs: &AttributeSet) -> Result<(), GeoTypeError> {
    buf.validate()?;
    attrs.validate(buf.len())
}

macro_rules! point_kind {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            frame: FrameRef,
            buf: PointBuffer,
            attrs: AttributeSet,
        }

        impl $name {
            /// An element in `frame` from a coordinate buffer (metres) and its attributes.
            /// Validates as described in the [module documentation](crate::points) and
            /// returns the first failure; nothing is changed or normalised.
            pub fn new(frame: FrameRef, buf: PointBuffer, attrs: AttributeSet) -> Result<$name, GeoTypeError> {
                validate(&buf, &attrs)?;
                Ok($name { frame, buf, attrs })
            }

            /// An element in `frame` from `[x, y, z]` points (metres), without attributes.
            pub fn from_points(frame: FrameRef, points: &[[f64; 3]]) -> Result<$name, GeoTypeError> {
                $name::new(frame, PointBuffer::from_points(points), AttributeSet::default())
            }

            /// The frame the coordinates are expressed in.
            pub fn frame(&self) -> FrameRef {
                self.frame
            }

            /// The number of points.
            pub fn len(&self) -> u64 {
                self.buf.len()
            }

            /// Whether there are no points.
            pub fn is_empty(&self) -> bool {
                self.buf.is_empty()
            }

            /// The coordinate buffer.
            pub fn buffer(&self) -> &PointBuffer {
                &self.buf
            }

            /// The attributes.
            pub fn attributes(&self) -> &AttributeSet {
                &self.attrs
            }

            /// The x coordinates, in index order.
            pub fn x(&self) -> &[f64] {
                &self.buf.x
            }

            /// The y coordinates, in index order.
            pub fn y(&self) -> &[f64] {
                &self.buf.y
            }

            /// The z coordinates, in index order.
            pub fn z(&self) -> &[f64] {
                &self.buf.z
            }

            /// Point `i`, if it exists.
            pub fn point(&self, i: u64) -> Option<[f64; 3]> {
                self.buf.point(i)
            }

            /// The bounding box; `Empty` when there are no points (REQ-GEO.TYPES.ENV-005).
            pub fn aabb(&self) -> Result<Aabb, GeoTypeError> {
                self.buf.aabb()
            }

            /// The frame, buffer and attributes, moved out.
            pub fn into_parts(self) -> (FrameRef, PointBuffer, AttributeSet) {
                (self.frame, self.buf, self.attrs)
            }

            /// A [`crate::PointSource`] over these points in blocks of `block_size`,
            /// in index order with original indexes (REQ-GEO.TYPES.ENV-006).
            pub fn source(&self, block_size: NonZeroUsize) -> InMemorySource<'_> {
                InMemorySource::new(self.frame, &self.buf, block_size)
            }
        }
    };
}

point_kind!(
    /// A set of sparse or probed points in one frame, with optional attributes.
    PointSet
);

point_kind!(
    /// A dense point cloud held in memory (scanner data), with optional attributes,
    /// including scanner structure. Distinct from [`PointSet`]; convert explicitly with
    /// [`PointCloud::to_point_set`].
    PointCloud
);

// AC-6: counts and indexes are u64 (checked at compile time).
const _: fn(&PointSet) -> u64 = PointSet::len;
const _: fn(&PointCloud) -> u64 = PointCloud::len;
const _: fn(&PointSet, u64) -> Option<[f64; 3]> = PointSet::point;
const _: fn(&PointCloud, u64) -> Option<[f64; 3]> = PointCloud::point;
const _: fn(&PointBuffer) -> u64 = PointBuffer::len;

impl PointCloud {
    /// The same points and attributes as a new [`PointSet`] in the same frame
    /// (REQ-GEO.TYPES.ENV-004).
    pub fn to_point_set(&self) -> PointSet {
        PointSet { frame: self.frame, buf: self.buf.clone(), attrs: self.attrs.clone() }
    }
}

impl PointSet {
    /// Crate-internal constructor from an already validated buffer, without attributes.
    pub(crate) fn from_validated(frame: FrameRef, buf: PointBuffer) -> PointSet {
        PointSet { frame, buf, attrs: AttributeSet::default() }
    }
}
