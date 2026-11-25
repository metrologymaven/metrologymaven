//! Normal inversion of a whole mesh or of a triangle selection (spec node
//! `SCAN.MESHOP.CUT.INVERT`), the kernel of `mesh.invert_normals@1`.
//!
//! The operation runs through the `SCAN.MESHOP.PROV` contract ([`Registered`]) and:
//!
//! * copies every input vertex first, in index order, so output vertex `i` is input vertex
//!   `i` bit for bit (an unreferenced vertex is kept too);
//! * copies every untargeted triangle with `copy_triangle` (REQ-001: unchanged, flag kept);
//! * writes every targeted triangle `(a, b, c)` as `(a, c, b)`, its second and third index
//!   swapped (REQ-001), through `derive_triangle` with that one triangle as its only source,
//!   so its `synthesized` flag is the source's flag (REQ-004: flags kept);
//! * negates a stored vertex normal exactly when the vertex has at least one incident
//!   triangle and every incident triangle is targeted (REQ-002). A triangle with a repeated
//!   index counts once for that vertex. An unreferenced vertex keeps its normal: it has no
//!   incident triangle whose orientation the normal could follow;
//! * keeps every other vertex channel, the scan origins, the grid shape and the
//!   per-triangle attributes unchanged (the triangle order is kept, so they still line up).
//!
//! The swap and the negation are involutions and nothing else changes, so inverting the
//! same target twice gives back the input's geometry, channels and flags exactly
//! (REQ-003). The version digest of the twice-inverted mesh equals the input's version
//! digest whenever the input carries the `synthesized` attribute (any output of an earlier
//! operation). A raw input without the attribute comes back with an all-false attribute,
//! so its element digest (`fid_geo_types::digest_mesh`) is reproduced but its version digest
//! is the derived-mesh digest: `SCAN.MESHOP.PROV` gives every operation output a digest no
//! raw mesh can have.
//!
//! # Target and stale selections
//!
//! [`InvertParams::whole`] targets every triangle. [`InvertParams::from_selection`] takes a
//! `GEO.SEL` index-set selection in the `triangles` domain and binds it to the element digest
//! of the mesh it was made on; a selection whose stored target digest differs from the
//! mesh's element digest fails with `scan.mesh.stale_selection`, and so does running the
//! parameters on any other mesh. The record's parameters are `target` (`all` or
//! `selection`) and, for a selection, its `GEO.SEL` content digest (`selection`) and member
//! count (`triangles`).
//!
//! Determinism class D0: integer work over vectors and exact sign flips; no float
//! arithmetic otherwise, no hash containers, no threads. Every allocation sized from the
//! input is fallible.

use fid_geo_sel::{Domain, IndexSet, Selection, SelectionBody};
use fid_geo_types::{digest_mesh, AttributeSet, ElementDigest, Mesh, TriangleAttrs};
use fid_types::{Determinism, Digest};

use crate::op::{
    MeshOp, MeshOpBuilder, MeshOpError, MeshOpOutput, OpParams, ParamsWriter, Registered, TriRef, VerifiedMeshView,
};

/// The registry command ID.
pub const INVERT_NORMALS: &str = "mesh.invert_normals@1";
/// The selection was made on another version of the mesh (or the parameters are run on a
/// mesh other than the one they were bound to).
pub const STALE_SELECTION: &str = "scan.mesh.stale_selection";
/// The selection is a query, not an index set.
pub const SELECTION_NOT_INDEX_SET: &str = "scan.mesh.selection_not_index_set";
/// The selection is not in the `triangles` domain.
pub const SELECTION_DOMAIN: &str = "scan.mesh.selection_domain";

fn op_error(code: &'static str) -> MeshOpError {
    MeshOpError::Op { code }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    All,
    Selected { set: IndexSet, bound: ElementDigest, digest: Digest },
}

/// What `mesh.invert_normals@1` inverts: every triangle, or the members of one selection
/// bound to the mesh it was made on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvertParams {
    target: Target,
}

impl InvertParams {
    /// Every triangle of the mesh.
    pub fn whole() -> InvertParams {
        InvertParams { target: Target::All }
    }

    /// The triangles of `selection`, checked against `mesh` in this order: a query body
    /// fails with `scan.mesh.selection_not_index_set`; a stored target digest other than
    /// the mesh's element digest with `scan.mesh.stale_selection`; a domain other than
    /// `triangles` with `scan.mesh.selection_domain`; a member not less than the triangle
    /// count with `scan.mesh.invalid_index`.
    pub fn from_selection(selection: &Selection, mesh: &Mesh) -> Result<InvertParams, MeshOpError> {
        let SelectionBody::IndexSet(set) = selection.body() else {
            return Err(op_error(SELECTION_NOT_INDEX_SET));
        };
        let found = digest_mesh(mesh);
        if selection.target_digest() != found {
            return Err(op_error(STALE_SELECTION));
        }
        if set.domain() != Domain::Triangles {
            return Err(op_error(SELECTION_DOMAIN));
        }
        set.check_bound(mesh.triangle_count())
            .map_err(|_| MeshOpError::InvalidIndex { input: 0, index: set.max_index().unwrap_or(0) })?;
        let digest = set.digest(found).to_digest();
        Ok(InvertParams { target: Target::Selected { set: set.clone(), bound: found, digest } })
    }

    /// Whether every triangle is targeted.
    pub fn is_whole(&self) -> bool {
        matches!(self.target, Target::All)
    }

    /// The `GEO.SEL` content digest of the selection (`None` for the whole mesh).
    pub fn selection_digest(&self) -> Option<Digest> {
        match &self.target {
            Target::All => None,
            Target::Selected { digest, .. } => Some(*digest),
        }
    }
}

impl OpParams for InvertParams {
    fn write(&self, w: &mut ParamsWriter) -> Result<(), MeshOpError> {
        match &self.target {
            Target::All => w.str("target", "all"),
            Target::Selected { set, digest, .. } => {
                w.str("target", "selection")?;
                w.str("selection", digest.as_str())?;
                w.u64("triangles", set.len())
            }
        }
    }
}

/// Which triangles an inversion reverses and which stored vertex normals it negates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvertPlan {
    targeted: Vec<bool>,
    negated: Vec<bool>,
}

impl InvertPlan {
    /// Whether triangle `t` is reversed (`None` when it does not exist).
    pub fn targeted(&self, t: u64) -> Option<bool> {
        usize::try_from(t).ok().and_then(|i| self.targeted.get(i)).copied()
    }

    /// Whether the normal of vertex `v` is negated, if the mesh stores normals (`None`
    /// when the vertex does not exist).
    pub fn negated(&self, v: u64) -> Option<bool> {
        usize::try_from(v).ok().and_then(|i| self.negated.get(i)).copied()
    }

    /// The number of reversed triangles.
    pub fn triangle_count(&self) -> u64 {
        self.targeted.iter().filter(|&&b| b).count() as u64
    }

    /// The number of vertices whose normal is negated when the mesh stores normals (all of
    /// its incident triangles are targeted and it has at least one).
    pub fn vertex_count(&self) -> u64 {
        self.negated.iter().filter(|&&b| b).count() as u64
    }
}

fn falses(n: u64) -> Result<Vec<bool>, MeshOpError> {
    let n = usize::try_from(n).map_err(|_| MeshOpError::AllocationFailed)?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| MeshOpError::AllocationFailed)?;
    v.resize(n, false);
    Ok(v)
}

fn zeros(n: u64) -> Result<Vec<u64>, MeshOpError> {
    let n = usize::try_from(n).map_err(|_| MeshOpError::AllocationFailed)?;
    let mut v = Vec::new();
    v.try_reserve_exact(n).map_err(|_| MeshOpError::AllocationFailed)?;
    v.resize(n, 0);
    Ok(v)
}

/// The plan of inverting `params`' target on `mesh`. Selection parameters bound to another
/// mesh fail with `scan.mesh.stale_selection`.
pub fn plan(mesh: &Mesh, params: &InvertParams) -> Result<InvertPlan, MeshOpError> {
    let n_tris = mesh.triangle_count();
    let mut targeted = falses(n_tris)?;
    match &params.target {
        Target::All => targeted.fill(true),
        Target::Selected { set, bound, .. } => {
            if digest_mesh(mesh) != *bound {
                return Err(op_error(STALE_SELECTION));
            }
            for &(start, end) in set.ranges() {
                for t in start..end {
                    let slot = usize::try_from(t)
                        .ok()
                        .and_then(|i| targeted.get_mut(i))
                        .ok_or(MeshOpError::InvalidIndex { input: 0, index: t })?;
                    *slot = true;
                }
            }
        }
    }
    let n_verts = mesh.vertex_count();
    let mut incident = zeros(n_verts)?;
    let mut incident_targeted = zeros(n_verts)?;
    for (t, tri) in mesh.triangles().enumerate() {
        let is_targeted = targeted.get(t).copied().unwrap_or(false);
        for (k, &v) in tri.iter().enumerate() {
            if tri[..k].contains(&v) {
                continue; // a repeated index counts once
            }
            let i = usize::try_from(v).map_err(|_| MeshOpError::InvalidIndex { input: 0, index: v })?;
            let inc = incident.get_mut(i).ok_or(MeshOpError::InvalidIndex { input: 0, index: v })?;
            *inc += 1;
            if is_targeted {
                if let Some(c) = incident_targeted.get_mut(i) {
                    *c += 1;
                }
            }
        }
    }
    let mut negated = falses(n_verts)?;
    for ((neg, &inc), &tgt) in negated.iter_mut().zip(&incident).zip(&incident_targeted) {
        *neg = inc > 0 && tgt == inc;
    }
    Ok(InvertPlan { targeted, negated })
}

fn try_copy<T: Clone>(v: &[T]) -> Result<Vec<T>, MeshOpError> {
    let mut out = Vec::new();
    out.try_reserve_exact(v.len()).map_err(|_| MeshOpError::AllocationFailed)?;
    out.extend_from_slice(v);
    Ok(out)
}

fn try_copy_opt<T: Clone>(v: &Option<Vec<T>>) -> Result<Option<Vec<T>>, MeshOpError> {
    v.as_deref().map(try_copy).transpose()
}

/// The input's vertex channels with the planned normals negated. The struct literal names
/// every field, so a channel added to GEO.TYPES fails to compile here until it is carried.
fn vertex_attrs(a: &AttributeSet, plan: &InvertPlan) -> Result<AttributeSet, MeshOpError> {
    let normal = match &a.normal {
        None => None,
        Some([nx, ny, nz]) => {
            let mut cols = [try_copy(nx)?, try_copy(ny)?, try_copy(nz)?];
            for col in cols.iter_mut() {
                for (x, &neg) in col.iter_mut().zip(&plan.negated) {
                    if neg {
                        *x = -*x;
                    }
                }
            }
            Some(cols)
        }
    };
    Ok(AttributeSet {
        normal,
        intensity: try_copy_opt(&a.intensity)?,
        colour: try_copy_opt(&a.colour)?,
        scanner_id: try_copy_opt(&a.scanner_id)?,
        timestamp_ns: try_copy_opt(&a.timestamp_ns)?,
        quality: try_copy_opt(&a.quality)?,
        scan_origins: try_copy(&a.scan_origins)?,
        grid: a.grid,
    })
}

fn triangle_attrs(a: &TriangleAttrs) -> Result<TriangleAttrs, MeshOpError> {
    Ok(TriangleAttrs { scanner_id: try_copy_opt(&a.scanner_id)?, quality: try_copy_opt(&a.quality)? })
}

/// The `mesh.invert_normals@1` mesh operation (D0, one input).
#[derive(Debug, Clone, Copy, Default)]
pub struct InvertNormals;

impl MeshOp for InvertNormals {
    const ID: &'static str = INVERT_NORMALS;
    const DETERMINISM: Determinism = Determinism::D0;
    type Params = InvertParams;

    fn run(
        &self,
        inputs: &[VerifiedMeshView<'_>],
        params: &InvertParams,
        out: &mut MeshOpBuilder<'_, '_>,
    ) -> Result<(), MeshOpError> {
        let [input] = inputs else {
            return Err(if inputs.is_empty() { MeshOpError::NoInput } else { MeshOpError::TooManyInputs });
        };
        let mesh = input.mesh();
        let plan = plan(mesh, params)?;
        for v in 0..mesh.vertex_count() {
            out.copy_vertex(0, v)?;
        }
        for (t, tri) in mesh.triangles().enumerate() {
            let src = TriRef::new(0, t as u64);
            if plan.targeted.get(t).copied().unwrap_or(false) {
                let [a, b, c] = tri;
                let (a, b, c) = (out.copy_vertex(0, a)?, out.copy_vertex(0, b)?, out.copy_vertex(0, c)?);
                out.derive_triangle(&[src], [a, c, b])?;
            } else {
                out.copy_triangle(src)?;
            }
        }
        out.set_vertex_attrs(vertex_attrs(mesh.vertex_attrs(), &plan)?);
        out.set_triangle_attrs(triangle_attrs(mesh.triangle_attrs())?);
        Ok(())
    }
}

/// Inverts `params`' target on `input` through the `SCAN.MESHOP.PROV` runner and returns the
/// new mesh version with its record. On any error nothing is returned.
pub fn invert_normals(input: VerifiedMeshView<'_>, params: &InvertParams) -> Result<MeshOpOutput, MeshOpError> {
    Registered::new(InvertNormals)?.run(&[input], params)
}
