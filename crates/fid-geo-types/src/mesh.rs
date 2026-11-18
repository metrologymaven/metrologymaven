//! Minimal indexed triangle mesh for the envelope (spec node `GEO.TYPES.ENV`).
//!
//! Only what the envelope needs: a frame, finite vertices and in-range triangle indexes.
//! Per-vertex attributes and topology facts are added by `GEO.TYPES.MESH`.

use crate::envelope::{Aabb, FrameRef};
use crate::error::GeoTypeError;
use crate::points::{PointBuffer, PointSet};

/// An indexed triangle mesh in one frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Mesh {
    frame: FrameRef,
    vertices: PointBuffer,
    triangles: Vec<[u32; 3]>,
}

impl Mesh {
    /// A mesh in `frame`. Fails on a non-finite vertex (`NonFinite`) or a triangle that
    /// refers to a missing vertex (`IndexOutOfRange`).
    pub fn new(frame: FrameRef, vertices: &[[f64; 3]], triangles: &[[u32; 3]]) -> Result<Mesh, GeoTypeError> {
        let vertices = PointBuffer::from_points(vertices);
        vertices.validate()?;
        let n = vertices.len();
        for (t, tri) in triangles.iter().enumerate() {
            for &v in tri {
                if u64::from(v) >= n {
                    return Err(GeoTypeError::IndexOutOfRange { triangle: t as u64, index: u64::from(v) });
                }
            }
        }
        Ok(Mesh { frame, vertices, triangles: triangles.to_vec() })
    }

    /// The frame the vertices are expressed in.
    pub fn frame(&self) -> FrameRef {
        self.frame
    }

    /// The number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.vertices.x.len()
    }

    /// Vertex `i`, if it exists.
    pub fn vertex(&self, i: usize) -> Option<[f64; 3]> {
        self.vertices.get(i)
    }

    /// The triangles as vertex-index triples.
    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    /// The bounding box of the vertices; `Empty` when there are none.
    pub fn aabb(&self) -> Result<Aabb, GeoTypeError> {
        self.vertices.aabb()
    }

    /// The vertices, in vertex order, as a new [`PointSet`] in the same frame
    /// (REQ-GEO.TYPES.ENV-004).
    pub fn vertices_to_point_set(&self) -> PointSet {
        PointSet::from_validated(self.frame, self.vertices.clone())
    }
}
