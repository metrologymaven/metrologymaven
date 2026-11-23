//! Shared mesh builder and import defect counter (spec node `IO.MESH.MBLD`).
//!
//! Accepts vertices and faces as read by any format parser, enforces `fid-io-reg`
//! [`Budget`] limits (`elements` and `alloc`), keeps every vertex and face in exact
//! input order without repair (REQ-IO.MESH.MBLD-001), validates index bounds
//! (REQ-IO.MESH.MBLD-003), counts degenerate faces, non-manifold edges and unreferenced
//! vertices exactly (REQ-IO.MESH.MBLD-002), and produces a validated GEO [`Mesh`] or
//! [`PointSet`].

use crate::attrs::{AttrTable, ScalarColumn};
use crate::error::MeshBuildError;
use core::mem::size_of;
use fid_geo_types::{FrameRef, Mesh, MeshOptions, PointBuffer, PointSet};
use fid_io_reg::{Budget, Position};
use std::collections::BTreeMap;

/// Precision of vertex coordinates in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourcePrecision {
    /// Single-precision binary32 coordinates widened losslessly to `f64`.
    F32,
    /// Double-precision binary64 coordinates.
    F64,
}

impl SourcePrecision {
    /// Stable lowercase token for import reports (`"f32"` or `"f64"`).
    pub const fn as_str(self) -> &'static str {
        match self {
            SourcePrecision::F32 => "f32",
            SourcePrecision::F64 => "f64",
        }
    }
}

/// Exact mesh import defect and element counts (REQ-IO.MESH.MBLD-002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshImportCounts {
    /// Total vertices kept (in input order).
    pub vertices: u64,
    /// Total triangle faces kept (in input order).
    pub faces: u64,
    /// Faces with a repeated vertex index or an exact zero `f64` cross product.
    pub degenerate_faces: u64,
    /// Undirected edges shared by more than two non-index-degenerate faces.
    pub non_manifold_edges: u64,
    /// Vertices not referenced by any face.
    pub unreferenced_vertices: u64,
}

/// Result of finalising a [`MeshBuilder`] into a GEO [`Mesh`].
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltMesh {
    /// Validated GEO triangle mesh with vertices and faces in exact input order.
    pub mesh: Mesh,
    /// Precision declared by the source format.
    pub source_precision: SourcePrecision,
    /// Exact import counts (vertices, faces, degenerate, non-manifold, unreferenced).
    pub counts: MeshImportCounts,
    /// Format-specific custom vertex scalar columns preserved losslessly.
    pub custom_attrs: BTreeMap<String, ScalarColumn>,
}

/// Result of finalising a vertex-only [`MeshBuilder`] into a GEO [`PointSet`].
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltPointSet {
    /// Validated GEO point set with vertices in exact input order.
    pub points: PointSet,
    /// Precision declared by the source format.
    pub source_precision: SourcePrecision,
    /// Exact import counts (`faces == 0`, `unreferenced_vertices == vertices`).
    pub counts: MeshImportCounts,
    /// Format-specific custom vertex scalar columns preserved losslessly.
    pub custom_attrs: BTreeMap<String, ScalarColumn>,
}

/// Output of [`MeshBuilder::build`]: a GEO triangle mesh when faces are present, or a
/// GEO point set for vertex-only inputs (such as vertex-only PLY files).
#[derive(Debug, Clone, PartialEq)]
pub enum BuiltGeometry {
    Mesh(BuiltMesh),
    PointSet(BuiltPointSet),
}

impl BuiltGeometry {
    /// Import counts common to both outputs.
    pub fn counts(&self) -> MeshImportCounts {
        match self {
            BuiltGeometry::Mesh(m) => m.counts,
            BuiltGeometry::PointSet(p) => p.counts,
        }
    }

    /// Source coordinate precision common to both outputs.
    pub fn source_precision(&self) -> SourcePrecision {
        match self {
            BuiltGeometry::Mesh(m) => m.source_precision,
            BuiltGeometry::PointSet(p) => p.source_precision,
        }
    }
}

/// Bytes per vertex stored in three `f64` columns.
const VERTEX_BYTES: u64 = 3 * size_of::<f64>() as u64;
/// Bytes per triangle face stored as `[u64; 3]`.
const FACE_BYTES: u64 = size_of::<[u64; 3]>() as u64;

fn count_of(n: usize) -> u64 {
    u64::try_from(n).unwrap_or(u64::MAX)
}

fn is_index_degenerate(f: [u64; 3]) -> bool {
    f[0] == f[1] || f[1] == f[2] || f[0] == f[2]
}

/// True iff the `f64` cross product `(b - a) x (c - a)` is identically zero (`[0.0, 0.0, 0.0]`).
/// Uses fixed subtraction/multiplication order with no fused multiply-add.
fn cross_is_exact_zero(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> bool {
    let ab0 = b[0] - a[0];
    let ab1 = b[1] - a[1];
    let ab2 = b[2] - a[2];
    let ac0 = c[0] - a[0];
    let ac1 = c[1] - a[1];
    let ac2 = c[2] - a[2];
    let cx = ab1 * ac2 - ab2 * ac1;
    let cy = ab2 * ac0 - ab0 * ac2;
    let cz = ab0 * ac1 - ab1 * ac0;
    cx == 0.0 && cy == 0.0 && cz == 0.0
}

/// Counts undirected edges shared by more than two non-index-degenerate faces by sorting
/// undirected edge keys `(min, max)` and counting runs of length > 2 (no `HashMap`, D0).
///
/// Allocates and charges `3 * non_index_degenerate * 16` bytes against `budget`.
pub fn count_non_manifold_edges(faces: &[[u64; 3]], budget: &mut Budget, at: Position) -> Result<u64, MeshBuildError> {
    let live = faces.iter().filter(|&&f| !is_index_degenerate(f)).count();
    let edge_count = live.checked_mul(3).ok_or(MeshBuildError::AllocationFailed { needed: u64::MAX })?;
    let bytes = count_of(edge_count).saturating_mul(size_of::<[u64; 2]>() as u64);
    budget.charge_alloc(bytes, at)?;

    let mut edges: Vec<[u64; 2]> = Vec::new();
    edges.try_reserve_exact(edge_count).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;

    for &f in faces {
        if is_index_degenerate(f) {
            continue;
        }
        for (u, v) in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
            edges.push(if u < v { [u, v] } else { [v, u] });
        }
    }
    edges.sort_unstable();

    let mut non_manifold = 0u64;
    for run in edges.chunk_by(|a, b| a == b) {
        if run.len() > 2 {
            non_manifold += 1;
        }
    }
    Ok(non_manifold)
}

/// Builder shared by all mesh format readers (`IO.MESH.MBLD`).
#[derive(Debug, Clone)]
pub struct MeshBuilder {
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    source_precision: SourcePrecision,
    faces: Vec<[u64; 3]>,
    attrs: AttrTable,
    charged_vertex_cap: usize,
    charged_face_cap: usize,
    precharged_elements: u64,
}

impl MeshBuilder {
    /// Creates an empty builder recording the source coordinate precision.
    pub fn new(source_precision: SourcePrecision) -> Self {
        MeshBuilder {
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            source_precision,
            faces: Vec::new(),
            attrs: AttrTable::new(),
            charged_vertex_cap: 0,
            charged_face_cap: 0,
            precharged_elements: 0,
        }
    }

    /// Source coordinate precision declared for this builder.
    pub fn source_precision(&self) -> SourcePrecision {
        self.source_precision
    }

    /// Number of vertices appended so far.
    pub fn vertex_count(&self) -> u64 {
        count_of(self.x.len())
    }

    /// Number of triangle faces appended so far.
    pub fn face_count(&self) -> u64 {
        count_of(self.faces.len())
    }

    /// Vertex `i` in metres, if present.
    pub fn vertex(&self, i: u64) -> Option<[f64; 3]> {
        let idx = usize::try_from(i).ok()?;
        Some([*self.x.get(idx)?, *self.y.get(idx)?, *self.z.get(idx)?])
    }

    /// Faces appended so far, in exact input order.
    pub fn faces(&self) -> &[[u64; 3]] {
        &self.faces
    }

    /// Read-only view of the per-vertex attribute table.
    pub fn attrs(&self) -> &AttrTable {
        &self.attrs
    }

    /// Mutable access to the per-vertex attribute table.
    pub fn attrs_mut(&mut self) -> &mut AttrTable {
        &mut self.attrs
    }

    /// Pre-charges `additional` vertices against `budget` (both `elements` and `alloc`)
    /// and reserves backing storage before parsing a header-declared count.
    pub fn reserve_vertices(
        &mut self,
        additional: u64,
        budget: &mut Budget,
        at: Position,
    ) -> Result<(), MeshBuildError> {
        if additional == 0 {
            return Ok(());
        }
        budget.charge_elements(additional, at)?;
        self.precharged_elements = self.precharged_elements.saturating_add(additional);

        let add_usize =
            usize::try_from(additional).map_err(|_| MeshBuildError::AllocationFailed { needed: u64::MAX })?;
        let target =
            self.x.len().checked_add(add_usize).ok_or(MeshBuildError::AllocationFailed { needed: u64::MAX })?;
        if target > self.charged_vertex_cap {
            let delta = target - self.charged_vertex_cap;
            let bytes = count_of(delta).saturating_mul(VERTEX_BYTES);
            budget.charge_alloc(bytes, at)?;
            self.x
                .try_reserve_exact(target - self.x.len())
                .map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
            self.y
                .try_reserve_exact(target - self.y.len())
                .map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
            self.z
                .try_reserve_exact(target - self.z.len())
                .map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
            self.charged_vertex_cap = self.x.capacity().min(self.y.capacity()).min(self.z.capacity());
        }
        Ok(())
    }

    /// Pre-charges `additional` faces against `budget` (both `elements` and `alloc`)
    /// and reserves backing storage before parsing a header-declared count.
    pub fn reserve_faces(&mut self, additional: u64, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        if additional == 0 {
            return Ok(());
        }
        budget.charge_elements(additional, at)?;
        self.precharged_elements = self.precharged_elements.saturating_add(additional);

        let add_usize =
            usize::try_from(additional).map_err(|_| MeshBuildError::AllocationFailed { needed: u64::MAX })?;
        let target =
            self.faces.len().checked_add(add_usize).ok_or(MeshBuildError::AllocationFailed { needed: u64::MAX })?;
        if target > self.charged_face_cap {
            let delta = target - self.charged_face_cap;
            let bytes = count_of(delta).saturating_mul(FACE_BYTES);
            budget.charge_alloc(bytes, at)?;
            self.faces
                .try_reserve_exact(target - self.faces.len())
                .map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
            self.charged_face_cap = self.faces.capacity();
        }
        Ok(())
    }

    fn consume_one_element(&mut self, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        if self.precharged_elements > 0 {
            self.precharged_elements -= 1;
            Ok(())
        } else {
            budget.charge_elements(1, at)?;
            Ok(())
        }
    }

    fn ensure_vertex_room(&mut self, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        if self.x.len() < self.charged_vertex_cap
            && self.x.len() < self.x.capacity()
            && self.y.len() < self.y.capacity()
            && self.z.len() < self.z.capacity()
        {
            return Ok(());
        }
        let grow_by = if self.x.capacity() == 0 { 64 } else { self.x.capacity() };
        let bytes = count_of(grow_by).saturating_mul(VERTEX_BYTES);
        budget.charge_alloc(bytes, at)?;
        self.x.try_reserve(grow_by).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
        self.y.try_reserve(grow_by).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
        self.z.try_reserve(grow_by).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
        self.charged_vertex_cap = self.x.capacity().min(self.y.capacity()).min(self.z.capacity());
        Ok(())
    }

    fn ensure_face_room(&mut self, budget: &mut Budget, at: Position) -> Result<(), MeshBuildError> {
        if self.faces.len() < self.charged_face_cap && self.faces.len() < self.faces.capacity() {
            return Ok(());
        }
        let grow_by = if self.faces.capacity() == 0 { 64 } else { self.faces.capacity() };
        let bytes = count_of(grow_by).saturating_mul(FACE_BYTES);
        budget.charge_alloc(bytes, at)?;
        self.faces.try_reserve(grow_by).map_err(|_| MeshBuildError::AllocationFailed { needed: bytes })?;
        self.charged_face_cap = self.faces.capacity();
        Ok(())
    }

    /// Appends one `f64` vertex position (in metres), keeping exact input order.
    /// Returns the assigned zero-based vertex index.
    pub fn push_vertex(&mut self, pos_m: [f64; 3], budget: &mut Budget, at: Position) -> Result<u64, MeshBuildError> {
        self.consume_one_element(budget, at)?;
        let idx = count_of(self.x.len());
        if !(pos_m[0].is_finite() && pos_m[1].is_finite() && pos_m[2].is_finite()) {
            return Err(MeshBuildError::NonFiniteVertex { vertex: idx });
        }
        self.ensure_vertex_room(budget, at)?;
        self.x.push(pos_m[0]);
        self.y.push(pos_m[1]);
        self.z.push(pos_m[2]);
        Ok(idx)
    }

    /// Appends one `f32` vertex position, widening each coordinate losslessly to `f64`
    /// before multiplying by `unit_to_metres` (REQ-IO.MESH.STL-003).
    pub fn push_vertex_f32(
        &mut self,
        pos: [f32; 3],
        unit_to_metres: f64,
        budget: &mut Budget,
        at: Position,
    ) -> Result<u64, MeshBuildError> {
        let widened = [
            f64::from(pos[0]) * unit_to_metres,
            f64::from(pos[1]) * unit_to_metres,
            f64::from(pos[2]) * unit_to_metres,
        ];
        self.push_vertex(widened, budget, at)
    }

    /// Appends one triangle face with `u32` vertex indices, keeping exact input order.
    /// Returns the assigned zero-based face index.
    pub fn push_face(&mut self, face: [u32; 3], budget: &mut Budget, at: Position) -> Result<u64, MeshBuildError> {
        self.push_face_u64([u64::from(face[0]), u64::from(face[1]), u64::from(face[2])], budget, at)
    }

    /// Appends one triangle face with `u64` vertex indices, keeping exact input order.
    /// Returns the assigned zero-based face index.
    pub fn push_face_u64(&mut self, face: [u64; 3], budget: &mut Budget, at: Position) -> Result<u64, MeshBuildError> {
        self.consume_one_element(budget, at)?;
        self.ensure_face_room(budget, at)?;
        let idx = count_of(self.faces.len());
        self.faces.push(face);
        Ok(idx)
    }

    /// Validates face indices and computes exact degenerate-face and unreferenced-vertex
    /// counts in one linear pass.
    fn scan_faces_and_vertices(&self, budget: &mut Budget, at: Position) -> Result<(u64, u64, u64), MeshBuildError> {
        let v_count = count_of(self.x.len());
        let word_count = self.x.len().div_ceil(64);
        let bitset_bytes = count_of(word_count).saturating_mul(size_of::<u64>() as u64);
        budget.charge_alloc(bitset_bytes, at)?;

        let mut referenced: Vec<u64> = Vec::new();
        referenced
            .try_reserve_exact(word_count)
            .map_err(|_| MeshBuildError::AllocationFailed { needed: bitset_bytes })?;
        referenced.resize(word_count, 0u64);

        let mut degenerate_faces = 0u64;
        let mut index_degenerate_faces = 0u64;

        for (face_idx, &tri) in self.faces.iter().enumerate() {
            for &vi in &tri {
                if vi >= v_count {
                    return Err(MeshBuildError::MalformedFace {
                        face: count_of(face_idx),
                        vertex_index: vi,
                        vertex_count: v_count,
                    });
                }
                if let Ok(u) = usize::try_from(vi) {
                    if let Some(word) = referenced.get_mut(u >> 6) {
                        *word |= 1u64 << (u & 63);
                    }
                }
            }

            if is_index_degenerate(tri) {
                degenerate_faces += 1;
                index_degenerate_faces += 1;
            } else {
                let pa = self.vertex(tri[0]).ok_or(MeshBuildError::MalformedFace {
                    face: count_of(face_idx),
                    vertex_index: tri[0],
                    vertex_count: v_count,
                })?;
                let pb = self.vertex(tri[1]).ok_or(MeshBuildError::MalformedFace {
                    face: count_of(face_idx),
                    vertex_index: tri[1],
                    vertex_count: v_count,
                })?;
                let pc = self.vertex(tri[2]).ok_or(MeshBuildError::MalformedFace {
                    face: count_of(face_idx),
                    vertex_index: tri[2],
                    vertex_count: v_count,
                })?;
                if cross_is_exact_zero(pa, pb, pc) {
                    degenerate_faces += 1;
                }
            }
        }

        let referenced_count: u64 = referenced.iter().map(|w| u64::from(w.count_ones())).sum();
        let unreferenced_vertices = v_count.saturating_sub(referenced_count);
        Ok((degenerate_faces, index_degenerate_faces, unreferenced_vertices))
    }

    /// Finalises into a GEO [`Mesh`], preserving every vertex and face in exact input
    /// order and computing exact [`MeshImportCounts`].
    pub fn build_mesh(self, frame: FrameRef, budget: &mut Budget, at: Position) -> Result<BuiltMesh, MeshBuildError> {
        let v_count = count_of(self.x.len());
        let f_count = count_of(self.faces.len());
        self.attrs.validate_custom_lengths(v_count)?;

        let (degenerate_faces, index_degenerate_faces, unreferenced_vertices) =
            self.scan_faces_and_vertices(budget, at)?;

        let use_u32 = v_count <= (1u64 << 32);
        let non_index_degenerate = f_count.saturating_sub(index_degenerate_faces);
        let work_bytes = Mesh::work_bytes(f_count, non_index_degenerate, use_u32)
            .ok_or(MeshBuildError::AllocationFailed { needed: u64::MAX })?;
        budget.charge_alloc(work_bytes, at)?;

        let buf = PointBuffer::new(self.x, self.y, self.z);
        let options = MeshOptions { work_budget_bytes: Some(work_bytes), ..MeshOptions::default() };
        let mesh = Mesh::with_options(frame, buf, self.attrs.standard, self.faces, options)?;

        let counts = MeshImportCounts {
            vertices: v_count,
            faces: f_count,
            degenerate_faces,
            non_manifold_edges: mesh.facts().nonmanifold_edges,
            unreferenced_vertices,
        };

        Ok(BuiltMesh { mesh, source_precision: self.source_precision, counts, custom_attrs: self.attrs.custom })
    }

    /// Finalises into a GEO [`PointSet`] for vertex-only inputs (faces must be empty).
    pub fn build_point_set(self, frame: FrameRef) -> Result<BuiltPointSet, MeshBuildError> {
        let v_count = count_of(self.x.len());
        self.attrs.validate_custom_lengths(v_count)?;
        let buf = PointBuffer::new(self.x, self.y, self.z);
        let points = PointSet::new(frame, buf, self.attrs.standard)?;
        let counts = MeshImportCounts {
            vertices: v_count,
            faces: 0,
            degenerate_faces: 0,
            non_manifold_edges: 0,
            unreferenced_vertices: v_count,
        };
        Ok(BuiltPointSet { points, source_precision: self.source_precision, counts, custom_attrs: self.attrs.custom })
    }

    /// Finalises into a GEO [`Mesh`] when at least one face was pushed, or a GEO
    /// [`PointSet`] when no faces were pushed.
    pub fn build(self, frame: FrameRef, budget: &mut Budget, at: Position) -> Result<BuiltGeometry, MeshBuildError> {
        if self.faces.is_empty() {
            Ok(BuiltGeometry::PointSet(self.build_point_set(frame)?))
        } else {
            Ok(BuiltGeometry::Mesh(self.build_mesh(frame, budget, at)?))
        }
    }
}
