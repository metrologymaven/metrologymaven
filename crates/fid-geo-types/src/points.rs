//! Minimal point set and point cloud storage for the envelope (spec node `GEO.TYPES.ENV`).
//!
//! Only what the envelope needs: a frame and finite f64 coordinates in metres, stored as
//! three arrays. Per-point attributes and the remaining invariants are added by
//! `GEO.TYPES.POINTS`.

use crate::envelope::{Aabb, FrameRef};
use crate::error::GeoTypeError;
use crate::source::InMemorySource;
use core::num::NonZeroUsize;

/// Finite coordinates as three equal-length arrays (structure of arrays).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Coords {
    pub(crate) x: Vec<f64>,
    pub(crate) y: Vec<f64>,
    pub(crate) z: Vec<f64>,
}

impl Coords {
    pub(crate) fn new(x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> Result<Coords, GeoTypeError> {
        let n = x.len();
        for (name, v) in [("y", &y), ("z", &z)] {
            if v.len() != n {
                return Err(GeoTypeError::AttributeLength {
                    name: name.to_string(),
                    expected: n as u64,
                    found: v.len() as u64,
                });
            }
        }
        for (i, ((a, b), c)) in x.iter().zip(&y).zip(&z).enumerate() {
            if !(a.is_finite() && b.is_finite() && c.is_finite()) {
                return Err(GeoTypeError::NonFinite { index: i as u64 });
            }
        }
        Ok(Coords { x, y, z })
    }

    pub(crate) fn from_points(points: &[[f64; 3]]) -> Result<Coords, GeoTypeError> {
        let x = points.iter().map(|p| p[0]).collect();
        let y = points.iter().map(|p| p[1]).collect();
        let z = points.iter().map(|p| p[2]).collect();
        Coords::new(x, y, z)
    }

    pub(crate) fn len(&self) -> usize {
        self.x.len()
    }

    pub(crate) fn point(&self, i: usize) -> Option<[f64; 3]> {
        Some([*self.x.get(i)?, *self.y.get(i)?, *self.z.get(i)?])
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = [f64; 3]> + '_ {
        self.x.iter().zip(&self.y).zip(&self.z).map(|((&a, &b), &c)| [a, b, c])
    }

    pub(crate) fn aabb(&self) -> Result<Aabb, GeoTypeError> {
        Aabb::of_points(self.iter())
    }
}

macro_rules! point_kind {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq)]
        pub struct $name {
            frame: FrameRef,
            coords: Coords,
        }

        impl $name {
            /// An element in `frame` from three coordinate arrays (metres). Fails on unequal
            /// lengths (`AttributeLength`) or a non-finite coordinate (`NonFinite`).
            pub fn new(frame: FrameRef, x: Vec<f64>, y: Vec<f64>, z: Vec<f64>) -> Result<$name, GeoTypeError> {
                Ok($name { frame, coords: Coords::new(x, y, z)? })
            }

            /// An element in `frame` from `[x, y, z]` points (metres).
            pub fn from_points(frame: FrameRef, points: &[[f64; 3]]) -> Result<$name, GeoTypeError> {
                Ok($name { frame, coords: Coords::from_points(points)? })
            }

            /// The frame the coordinates are expressed in.
            pub fn frame(&self) -> FrameRef {
                self.frame
            }

            /// The number of points.
            pub fn len(&self) -> usize {
                self.coords.len()
            }

            /// Whether there are no points.
            pub fn is_empty(&self) -> bool {
                self.coords.len() == 0
            }

            /// The x coordinates, in index order.
            pub fn x(&self) -> &[f64] {
                &self.coords.x
            }

            /// The y coordinates, in index order.
            pub fn y(&self) -> &[f64] {
                &self.coords.y
            }

            /// The z coordinates, in index order.
            pub fn z(&self) -> &[f64] {
                &self.coords.z
            }

            /// Point `i`, if it exists.
            pub fn point(&self, i: usize) -> Option<[f64; 3]> {
                self.coords.point(i)
            }

            /// The bounding box; `Empty` when there are no points (REQ-GEO.TYPES.ENV-005).
            pub fn aabb(&self) -> Result<Aabb, GeoTypeError> {
                self.coords.aabb()
            }

            /// A [`crate::PointSource`] over these points in blocks of `block_size`,
            /// in index order with original indexes (REQ-GEO.TYPES.ENV-006).
            pub fn source(&self, block_size: NonZeroUsize) -> InMemorySource<'_> {
                InMemorySource::new(self.frame, &self.coords, block_size)
            }
        }
    };
}

point_kind!(
    /// An unstructured set of points in one frame.
    PointSet
);

point_kind!(
    /// A point cloud held in memory (scanner data). Distinct from [`PointSet`]; convert
    /// explicitly with [`PointCloud::to_point_set`].
    PointCloud
);

impl PointCloud {
    /// The same points as a new [`PointSet`] in the same frame (REQ-GEO.TYPES.ENV-004).
    pub fn to_point_set(&self) -> PointSet {
        PointSet { frame: self.frame, coords: self.coords.clone() }
    }
}

impl PointSet {
    /// Crate-internal constructor from already validated coordinates.
    pub(crate) fn from_coords(frame: FrameRef, coords: Coords) -> PointSet {
        PointSet { frame, coords }
    }
}
