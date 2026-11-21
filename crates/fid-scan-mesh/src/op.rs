//! The mesh-operation contract and the synthesized-triangle flag (spec node
//! `SCAN.MESHOP.PROV`).
//!
//! Every mesh operation of `SCAN.MESHOP` implements [`MeshOp`] and runs only through
//! [`Registered::run`], which owns everything the operation must not be able to get wrong:
//!
//! * **Registration** ([`Registered::new`]) refuses any determinism class other than D0
//!   and D2 (`scan.mesh.bad_determinism`, REQ-002) and an operation ID that is not a
//!   command ID `name@major` (`scan.mesh.bad_op_id`). An unregistered operation cannot run.
//! * **Inputs are read-only** (REQ-001): the operation receives `&[MeshView]`, shared
//!   references to the input meshes and their flags. After the run every input's digest is
//!   recomputed and compared with the one recorded before it ([`check_input_intact`],
//!   `scan.mesh.input_mutated`), so the record can never cite an input that changed.
//! * **Output only through the builder** ([`MeshOpBuilder`]): `copy_triangle` keeps the
//!   input flag and the input vertex positions exactly, `derive_triangle` sets the OR of
//!   its (at least one) contributing input triangles, and `create_triangle` sets true
//!   (REQ-003). Inputs without the attribute count as all-false (REQ-004). The operation
//!   never sets a flag directly.
//! * **The record** ([`MeshOpOutput`]) is assembled by the runner, not the operation: the
//!   operation ID, determinism class, the digest of every input version in order, the
//!   canonical parameter JSON ([`OpParams`]), the seed (required for D2, refused for D0),
//!   the `SCAN.POLY.QUAL` report of every input and of the output (REQ-005), the output
//!   digest and the synthesized-triangle count.
//!
//! # Digests
//!
//! The version digest of a mesh **without** the attribute is its GEO.TYPES element digest
//! ([`fid_geo_types::digest_mesh`]). The version digest of a mesh **with** the attribute,
//! which every output has, is the *derived-mesh digest*: SHA-256 (FIPS 180-4, `fid-kci`)
//! of the layout
//!
//! ```text
//! "FIDMOP" | 0x01 | element digest [32 raw bytes] | n_tris u64 LE | flags [ceil(n_tris / 8)]
//! ```
//!
//! with the flags packed as in [`Synthesized::to_bytes`]. The prefix differs from the
//! `"FIDGEO"` element layout, so a derived mesh never has the digest of a raw, measured
//! mesh, even when its geometry is bit-identical (an identity operation): re-wrapping an
//! output's mesh without its flags gives a different digest, and
//! [`MeshView::verified`] against the recorded output digest fails with
//! `scan.mesh.digest_mismatch`. Callers that load a mesh version from storage wrap it with
//! [`MeshView::verified`] so the digest they were given is checked against the content.
//!
//! The record's canonical JSON (kernel canonical JSON: keys in byte order, numbers as
//! strings) has the keys `determinism`, `inputs`, `kind` (`scan.mesh_op_record@1`), `op`,
//! `output`, `params`, `quality_after`, `quality_before` (one report per input),
//! `seed` (decimal string or `null`), `synthesized` and `triangles`; its SHA-256 is
//! [`MeshOpOutput::record_digest`].
//!
//! # Bounds and determinism
//!
//! At most [`MAX_INPUTS`] inputs, all in one frame. Output vertex and triangle counts are
//! bounded by [`MeshOpLimits`]; every allocation sized from the input is fallible. The
//! contract itself is D0: integer work over vectors (no hash containers), copied
//! coordinates unchanged, no float arithmetic, no threads.

use core::fmt;

use fid_geo_types::{digest_mesh, AttributeSet, GeoTypeError, Mesh, MeshOptions, PointBuffer};
use fid_kci::kci::{sha256, Sha256};
use fid_types::{CommandId, Determinism, Digest};

use crate::quality::{mesh_quality, MeshQualityReport, QualityError, DEFAULT_DEGENERATE_AREA};

mod params;
mod synth;

pub use params::{OpParams, ParamsWriter, MAX_KEY_BYTES, MAX_PARAMS_BYTES, MAX_PARAM_ENTRIES};
pub use synth::Synthesized;

/// The most input meshes one operation may take.
pub const MAX_INPUTS: usize = 64;
/// The `kind` of the record JSON.
pub const RECORD_KIND: &str = "scan.mesh_op_record@1";
/// The tag that starts the derived-mesh digest layout.
pub const DERIVED_TAG: [u8; 6] = *b"FIDMOP";
/// The derived-mesh digest layout version.
pub const DERIVED_VERSION: u8 = 1;

/// Every error of the contract. Errors are values; nothing here panics.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshOpError {
    /// The declared determinism class is not D0 or D2. Code `scan.mesh.bad_determinism`.
    BadDeterminism(Determinism),
    /// The operation ID is not a command ID `name@major`. Code `scan.mesh.bad_op_id`.
    BadOpId,
    /// A D2 operation's parameters carry no seed. Code `scan.mesh.missing_seed`.
    MissingSeed,
    /// A D0 operation's parameters carry a seed. Code `scan.mesh.unexpected_seed`.
    UnexpectedSeed,
    /// A parameter could not be recorded canonically. Code `scan.mesh.bad_params`.
    BadParams { reason: &'static str },
    /// No input mesh. Code `scan.mesh.no_input`.
    NoInput,
    /// More than [`MAX_INPUTS`] inputs. Code `scan.mesh.too_many_inputs`.
    TooManyInputs,
    /// The inputs are not all in one frame. Code `scan.mesh.frame_mismatch`.
    FrameMismatch { input: u64 },
    /// An input, input triangle, input vertex or output vertex that does not exist. Code
    /// `scan.mesh.invalid_index`.
    InvalidIndex { input: u64, index: u64 },
    /// `derive_triangle` without a contributing triangle. Code `scan.mesh.no_source`.
    NoSource,
    /// A new vertex has a NaN or infinite coordinate. Code `scan.mesh.non_finite`.
    NonFinite,
    /// A flag set whose length is not the mesh's triangle count. Code
    /// `scan.mesh.attribute_length`.
    AttributeLength { expected: u64, found: u64 },
    /// A mesh's content digest is not the expected one. Code `scan.mesh.digest_mismatch`.
    DigestMismatch,
    /// An input's digest changed during the run. Code `scan.mesh.input_mutated`.
    InputMutated { input: u64 },
    /// The output exceeds a [`MeshOpLimits`] bound. Code `scan.mesh.limit_exceeded`.
    LimitExceeded,
    /// A working allocation failed. Code `scan.mesh.allocation_failed`.
    AllocationFailed,
    /// The quality report failed (its own code).
    Quality(QualityError),
    /// GEO.TYPES refused the output mesh. Code `scan.mesh.invalid_output`.
    Geometry(GeoTypeError),
    /// The operation itself failed with its own stable code.
    Op { code: &'static str },
}

impl MeshOpError {
    /// The stable error code.
    pub fn code(&self) -> &'static str {
        match self {
            MeshOpError::BadDeterminism(_) => "scan.mesh.bad_determinism",
            MeshOpError::BadOpId => "scan.mesh.bad_op_id",
            MeshOpError::MissingSeed => "scan.mesh.missing_seed",
            MeshOpError::UnexpectedSeed => "scan.mesh.unexpected_seed",
            MeshOpError::BadParams { .. } => "scan.mesh.bad_params",
            MeshOpError::NoInput => "scan.mesh.no_input",
            MeshOpError::TooManyInputs => "scan.mesh.too_many_inputs",
            MeshOpError::FrameMismatch { .. } => "scan.mesh.frame_mismatch",
            MeshOpError::InvalidIndex { .. } => "scan.mesh.invalid_index",
            MeshOpError::NoSource => "scan.mesh.no_source",
            MeshOpError::NonFinite => "scan.mesh.non_finite",
            MeshOpError::AttributeLength { .. } => "scan.mesh.attribute_length",
            MeshOpError::DigestMismatch => "scan.mesh.digest_mismatch",
            MeshOpError::InputMutated { .. } => "scan.mesh.input_mutated",
            MeshOpError::LimitExceeded => "scan.mesh.limit_exceeded",
            MeshOpError::AllocationFailed => "scan.mesh.allocation_failed",
            MeshOpError::Quality(e) => e.code(),
            MeshOpError::Geometry(_) => "scan.mesh.invalid_output",
            MeshOpError::Op { code } => code,
        }
    }
}

impl fmt::Display for MeshOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MeshOpError::BadDeterminism(d) => write!(f, "{}: {d}", self.code()),
            MeshOpError::BadParams { reason } => write!(f, "{}: {reason}", self.code()),
            MeshOpError::InvalidIndex { input, index } => write!(f, "{}: input {input}, index {index}", self.code()),
            MeshOpError::AttributeLength { expected, found } => {
                write!(f, "{}: expected {expected}, found {found}", self.code())
            }
            MeshOpError::Quality(e) => write!(f, "{e}"),
            MeshOpError::Geometry(e) => write!(f, "{}: {e}", self.code()),
            _ => f.write_str(self.code()),
        }
    }
}

impl std::error::Error for MeshOpError {}

impl From<QualityError> for MeshOpError {
    fn from(e: QualityError) -> Self {
        MeshOpError::Quality(e)
    }
}

/// Checks a declared determinism class: D0, or D2 (whose seed the runner then requires).
pub fn check_determinism(d: Determinism) -> Result<(), MeshOpError> {
    match d {
        Determinism::D0 | Determinism::D2 => Ok(()),
        other => Err(MeshOpError::BadDeterminism(other)),
    }
}

/// The derived-mesh digest of `mesh` with `flags` (layout in the
/// [module documentation](self)).
pub fn derived_digest(mesh: &Mesh, flags: &Synthesized) -> Result<Digest, MeshOpError> {
    if flags.len() != mesh.triangle_count() {
        return Err(MeshOpError::AttributeLength { expected: mesh.triangle_count(), found: flags.len() });
    }
    let mut h = Sha256::new();
    h.update(&DERIVED_TAG);
    h.update(&[DERIVED_VERSION]);
    h.update(&digest_mesh(mesh).to_bytes());
    h.update(&flags.len().to_le_bytes());
    h.update(&flags.to_bytes()?);
    Ok(Digest::from_bytes(h.finish()))
}

fn view_digest(mesh: &Mesh, flags: Option<&Synthesized>) -> Result<Digest, MeshOpError> {
    match flags {
        None => Ok(digest_mesh(mesh).to_digest()),
        Some(f) => derived_digest(mesh, f),
    }
}

/// A read-only input of an operation: a mesh, its optional `synthesized` attribute and its
/// version digest (computed from the content, never taken on trust).
#[derive(Debug, Clone, Copy)]
pub struct MeshView<'a> {
    mesh: &'a Mesh,
    flags: Option<&'a Synthesized>,
    digest: Digest,
}

impl<'a> MeshView<'a> {
    /// A mesh without the attribute: every triangle counts as measured (REQ-004). Its
    /// digest is the GEO.TYPES element digest.
    pub fn unflagged(mesh: &'a Mesh) -> MeshView<'a> {
        MeshView { mesh, flags: None, digest: digest_mesh(mesh).to_digest() }
    }

    /// A mesh with its attribute; `flags` must have one value per triangle.
    pub fn flagged(mesh: &'a Mesh, flags: &'a Synthesized) -> Result<MeshView<'a>, MeshOpError> {
        let digest = derived_digest(mesh, flags)?;
        Ok(MeshView { mesh, flags: Some(flags), digest })
    }

    /// The output of an earlier operation, flags included (its digest is recomputed and
    /// equals [`MeshOpOutput::output_digest`]).
    pub fn of_output(out: &'a MeshOpOutput) -> Result<MeshView<'a>, MeshOpError> {
        MeshView::flagged(&out.mesh, &out.flags)
    }

    /// This view if its content digest is `expected`, else `scan.mesh.digest_mismatch`.
    /// Use it for every mesh version loaded by digest.
    pub fn verified(self, expected: &Digest) -> Result<MeshView<'a>, MeshOpError> {
        if self.digest == *expected {
            Ok(self)
        } else {
            Err(MeshOpError::DigestMismatch)
        }
    }

    /// The mesh.
    pub fn mesh(&self) -> &'a Mesh {
        self.mesh
    }

    /// The version digest.
    pub fn digest(&self) -> Digest {
        self.digest
    }

    /// Whether the mesh carries the attribute.
    pub fn has_attribute(&self) -> bool {
        self.flags.is_some()
    }

    /// The flag of triangle `t` (false for every triangle of an unflagged mesh); `None`
    /// when the triangle does not exist.
    pub fn synthesized(&self, t: u64) -> Option<bool> {
        if t >= self.mesh.triangle_count() {
            return None;
        }
        match self.flags {
            None => Some(false),
            Some(f) => f.get(t),
        }
    }
}

/// Recomputes the digest of `view`'s content and compares it with `before` (the input
/// integrity check every operation test, and the runner, uses).
pub fn check_input_intact(view: &MeshView<'_>, before: &Digest) -> Result<(), MeshOpError> {
    if view_digest(view.mesh, view.flags)? == *before {
        Ok(())
    } else {
        Err(MeshOpError::DigestMismatch)
    }
}

/// One triangle of one input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriRef {
    /// The input's position in the input list.
    pub input: usize,
    /// The triangle's index in that input.
    pub triangle: u64,
}

impl TriRef {
    /// Triangle `triangle` of input `input`.
    pub fn new(input: usize, triangle: u64) -> TriRef {
        TriRef { input, triangle }
    }
}

/// Bounds on the output (untrusted inputs and operations never allocate past them).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeshOpLimits {
    /// The most output vertices.
    pub max_vertices: u64,
    /// The most output triangles.
    pub max_triangles: u64,
    /// Passed to GEO.TYPES as the output mesh's working-memory budget.
    pub work_budget_bytes: Option<u64>,
}

impl Default for MeshOpLimits {
    /// 2³⁰ vertices and 2³⁰ triangles, no extra byte budget.
    fn default() -> Self {
        MeshOpLimits { max_vertices: 1 << 30, max_triangles: 1 << 30, work_budget_bytes: None }
    }
}

/// Options of one run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunOptions {
    /// Output bounds.
    pub limits: MeshOpLimits,
    /// The degenerate-area threshold of the quality reports (m²).
    pub degenerate_area: f64,
}

impl Default for RunOptions {
    fn default() -> Self {
        RunOptions { limits: MeshOpLimits::default(), degenerate_area: DEFAULT_DEGENERATE_AREA }
    }
}

const UNMAPPED: u64 = u64::MAX;

/// Builds the output mesh. Flags follow from the method used, never from the operation.
#[derive(Debug)]
pub struct MeshOpBuilder<'b, 'a> {
    inputs: &'b [MeshView<'a>],
    limits: MeshOpLimits,
    x: Vec<f64>,
    y: Vec<f64>,
    z: Vec<f64>,
    triangles: Vec<[u64; 3]>,
    flags: Synthesized,
    /// Per input, the output index of each copied vertex (allocated on first copy).
    vmap: Vec<Vec<u64>>,
}

fn grow<T>(v: &mut Vec<T>) -> Result<(), MeshOpError> {
    v.try_reserve(1).map_err(|_| MeshOpError::AllocationFailed)
}

impl<'b, 'a> MeshOpBuilder<'b, 'a> {
    fn new(inputs: &'b [MeshView<'a>], limits: MeshOpLimits) -> Result<Self, MeshOpError> {
        let mut vmap = Vec::new();
        vmap.try_reserve_exact(inputs.len()).map_err(|_| MeshOpError::AllocationFailed)?;
        vmap.resize_with(inputs.len(), Vec::new);
        Ok(MeshOpBuilder {
            inputs,
            limits,
            x: Vec::new(),
            y: Vec::new(),
            z: Vec::new(),
            triangles: Vec::new(),
            flags: Synthesized::new(),
            vmap,
        })
    }

    /// Output vertices so far.
    pub fn vertex_count(&self) -> u64 {
        self.x.len() as u64
    }

    /// Output triangles so far.
    pub fn triangle_count(&self) -> u64 {
        self.triangles.len() as u64
    }

    fn input(&self, input: usize) -> Result<&MeshView<'a>, MeshOpError> {
        self.inputs.get(input).ok_or(MeshOpError::InvalidIndex { input: input as u64, index: 0 })
    }

    fn push_vertex(&mut self, p: [f64; 3]) -> Result<u64, MeshOpError> {
        if self.vertex_count() >= self.limits.max_vertices {
            return Err(MeshOpError::LimitExceeded);
        }
        grow(&mut self.x)?;
        grow(&mut self.y)?;
        grow(&mut self.z)?;
        self.x.push(p[0]);
        self.y.push(p[1]);
        self.z.push(p[2]);
        Ok(self.vertex_count() - 1)
    }

    /// Copies vertex `v` of input `input` (its position bit for bit) and returns its output
    /// index; copying the same input vertex again returns the same index.
    pub fn copy_vertex(&mut self, input: usize, v: u64) -> Result<u64, MeshOpError> {
        let view = *self.input(input)?;
        let p = view.mesh.vertex(v).ok_or(MeshOpError::InvalidIndex { input: input as u64, index: v })?;
        let n = usize::try_from(view.mesh.vertex_count()).map_err(|_| MeshOpError::AllocationFailed)?;
        let vi = usize::try_from(v).map_err(|_| MeshOpError::InvalidIndex { input: input as u64, index: v })?;
        let map = self.vmap.get_mut(input).ok_or(MeshOpError::InvalidIndex { input: input as u64, index: v })?;
        if map.is_empty() {
            map.try_reserve_exact(n).map_err(|_| MeshOpError::AllocationFailed)?;
            map.resize(n, UNMAPPED);
        }
        if let Some(&o) = map.get(vi) {
            if o != UNMAPPED {
                return Ok(o);
            }
        }
        let o = self.push_vertex(p)?;
        if let Some(slot) = self.vmap.get_mut(input).and_then(|m| m.get_mut(vi)) {
            *slot = o;
        }
        Ok(o)
    }

    /// Adds a new vertex (for example a split midpoint or a moved position). Coordinates
    /// must be finite.
    pub fn add_vertex(&mut self, p: [f64; 3]) -> Result<u64, MeshOpError> {
        if !p.iter().all(|c| c.is_finite()) {
            return Err(MeshOpError::NonFinite);
        }
        self.push_vertex(p)
    }

    fn source_flag(&self, src: TriRef) -> Result<bool, MeshOpError> {
        let view = self.input(src.input)?;
        view.synthesized(src.triangle).ok_or(MeshOpError::InvalidIndex { input: src.input as u64, index: src.triangle })
    }

    fn push_triangle(&mut self, verts: [u64; 3], flag: bool) -> Result<u64, MeshOpError> {
        for &v in &verts {
            if v >= self.vertex_count() {
                return Err(MeshOpError::InvalidIndex { input: u64::MAX, index: v });
            }
        }
        if self.triangle_count() >= self.limits.max_triangles {
            return Err(MeshOpError::LimitExceeded);
        }
        grow(&mut self.triangles)?;
        self.flags.reserve(1)?;
        self.triangles.push(verts);
        self.flags.push(flag)?;
        Ok(self.triangle_count() - 1)
    }

    /// Copies an input triangle unchanged: its three vertices through
    /// [`copy_vertex`](Self::copy_vertex) and its flag as it was.
    pub fn copy_triangle(&mut self, src: TriRef) -> Result<u64, MeshOpError> {
        let flag = self.source_flag(src)?;
        let view = *self.input(src.input)?;
        let tri = view
            .mesh
            .triangle(src.triangle)
            .ok_or(MeshOpError::InvalidIndex { input: src.input as u64, index: src.triangle })?;
        let mut out = [0u64; 3];
        for (o, &v) in out.iter_mut().zip(tri.iter()) {
            *o = self.copy_vertex(src.input, v)?;
        }
        self.push_triangle(out, flag)
    }

    /// A triangle produced from input triangles (split, collapse, moved vertices): flagged
    /// synthesized if any of `sources` is. `verts` are output vertex indexes. At least one
    /// source is required.
    pub fn derive_triangle(&mut self, sources: &[TriRef], verts: [u64; 3]) -> Result<u64, MeshOpError> {
        if sources.is_empty() {
            return Err(MeshOpError::NoSource);
        }
        let mut flag = false;
        for &s in sources {
            flag |= self.source_flag(s)?;
        }
        self.push_triangle(verts, flag)
    }

    /// A triangle that bridges or fills area no input triangle covers: always synthesized.
    pub fn create_triangle(&mut self, verts: [u64; 3]) -> Result<u64, MeshOpError> {
        self.push_triangle(verts, true)
    }
}

/// A mesh operation. Implementations are pure functions of their inputs and parameters
/// (plus the recorded seed for D2) and write their result only through the builder.
pub trait MeshOp {
    /// The registry command ID, `name@major`.
    const ID: &'static str;
    /// The determinism class: D0, or D2 with a seed in the parameters.
    const DETERMINISM: Determinism;
    /// The parameters.
    type Params: OpParams;

    /// Runs the operation on read-only inputs.
    fn run(
        &self,
        inputs: &[MeshView<'_>],
        params: &Self::Params,
        out: &mut MeshOpBuilder<'_, '_>,
    ) -> Result<(), MeshOpError>;
}

/// An operation whose declaration passed registration; the only way to run one.
pub struct Registered<O: MeshOp> {
    op: O,
    id: CommandId,
}

impl<O: MeshOp> fmt::Debug for Registered<O> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registered").field("id", &self.id).field("determinism", &O::DETERMINISM).finish()
    }
}

impl<O: MeshOp> Registered<O> {
    /// Registers `op`: its determinism class must be D0 or D2 and its ID a command ID.
    pub fn new(op: O) -> Result<Registered<O>, MeshOpError> {
        check_determinism(O::DETERMINISM)?;
        let id = CommandId::parse(O::ID).map_err(|_| MeshOpError::BadOpId)?;
        Ok(Registered { op, id })
    }

    /// The operation ID.
    pub fn id(&self) -> &CommandId {
        &self.id
    }

    /// Runs with [`RunOptions::default`].
    pub fn run(&self, inputs: &[MeshView<'_>], params: &O::Params) -> Result<MeshOpOutput, MeshOpError> {
        self.run_with(inputs, params, &RunOptions::default())
    }

    /// Runs the operation and assembles its result and record. On any error no output is
    /// returned.
    pub fn run_with(
        &self,
        inputs: &[MeshView<'_>],
        params: &O::Params,
        options: &RunOptions,
    ) -> Result<MeshOpOutput, MeshOpError> {
        let first = inputs.first().ok_or(MeshOpError::NoInput)?;
        if inputs.len() > MAX_INPUTS {
            return Err(MeshOpError::TooManyInputs);
        }
        let frame = first.mesh.frame();
        for (i, v) in inputs.iter().enumerate() {
            if v.mesh.frame() != frame {
                return Err(MeshOpError::FrameMismatch { input: i as u64 });
            }
        }
        let seed = params.seed();
        match (O::DETERMINISM, seed) {
            (Determinism::D2, None) => return Err(MeshOpError::MissingSeed),
            (Determinism::D0, Some(_)) => return Err(MeshOpError::UnexpectedSeed),
            _ => {}
        }
        let mut w = ParamsWriter::new();
        params.write(&mut w)?;
        let params_json = w.finish()?;

        let mut input_digests = Vec::new();
        input_digests.try_reserve_exact(inputs.len()).map_err(|_| MeshOpError::AllocationFailed)?;
        let mut quality_before = Vec::new();
        quality_before.try_reserve_exact(inputs.len()).map_err(|_| MeshOpError::AllocationFailed)?;
        for v in inputs {
            input_digests.push(v.digest);
            quality_before.push(mesh_quality(v.mesh, options.degenerate_area)?);
        }

        let mut b = MeshOpBuilder::new(inputs, options.limits)?;
        self.op.run(inputs, params, &mut b)?;

        for (i, (v, d)) in inputs.iter().zip(&input_digests).enumerate() {
            check_input_intact(v, d).map_err(|_| MeshOpError::InputMutated { input: i as u64 })?;
        }

        let MeshOpBuilder { x, y, z, triangles, flags, .. } = b;
        let opts = MeshOptions { work_budget_bytes: options.limits.work_budget_bytes, ..MeshOptions::default() };
        let mesh = Mesh::with_options(frame, PointBuffer::new(x, y, z), AttributeSet::default(), triangles, opts)
            .map_err(MeshOpError::Geometry)?;
        let quality_after = mesh_quality(&mesh, options.degenerate_area)?;
        let output = derived_digest(&mesh, &flags)?;
        let mut out = MeshOpOutput {
            mesh,
            flags,
            op: self.id.clone(),
            determinism: O::DETERMINISM,
            inputs: input_digests,
            params_json,
            seed,
            quality_before,
            quality_after,
            output,
            record_json: Vec::new(),
            record_digest: Digest::TOMBSTONE,
        };
        out.record_json = out.render_record()?;
        out.record_digest = Digest::from_bytes(sha256(&out.record_json));
        Ok(out)
    }
}

/// The result of a run: the new mesh version with its flags, and its provenance record.
/// Fields are read-only; only [`Registered::run_with`] makes one.
#[derive(Debug, Clone)]
pub struct MeshOpOutput {
    mesh: Mesh,
    flags: Synthesized,
    op: CommandId,
    determinism: Determinism,
    inputs: Vec<Digest>,
    params_json: Vec<u8>,
    seed: Option<u64>,
    quality_before: Vec<MeshQualityReport>,
    quality_after: MeshQualityReport,
    output: Digest,
    record_json: Vec<u8>,
    record_digest: Digest,
}

impl MeshOpOutput {
    /// The new mesh. Its version identity is [`output_digest`](Self::output_digest), which
    /// covers the flags; the bare mesh must not be passed on without them.
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }

    /// The per-triangle `synthesized` flags (one per output triangle).
    pub fn synthesized(&self) -> &Synthesized {
        &self.flags
    }

    /// The operation ID.
    pub fn op(&self) -> &CommandId {
        &self.op
    }

    /// The declared determinism class.
    pub fn determinism(&self) -> Determinism {
        self.determinism
    }

    /// The digests of the input versions, in input order.
    pub fn inputs(&self) -> &[Digest] {
        &self.inputs
    }

    /// The canonical parameter object.
    pub fn params_json(&self) -> &[u8] {
        &self.params_json
    }

    /// The seed (D2 only).
    pub fn seed(&self) -> Option<u64> {
        self.seed
    }

    /// The quality report of each input, in input order.
    pub fn quality_before(&self) -> &[MeshQualityReport] {
        &self.quality_before
    }

    /// The quality report of the output.
    pub fn quality_after(&self) -> &MeshQualityReport {
        &self.quality_after
    }

    /// The derived-mesh digest of the output (mesh and flags).
    pub fn output_digest(&self) -> Digest {
        self.output
    }

    /// The record as canonical JSON.
    pub fn record_json(&self) -> &[u8] {
        &self.record_json
    }

    /// SHA-256 of [`record_json`](Self::record_json).
    pub fn record_digest(&self) -> Digest {
        self.record_digest
    }

    fn render_record(&self) -> Result<Vec<u8>, MeshOpError> {
        let mut o = Vec::new();
        let put = |o: &mut Vec<u8>, s: &[u8]| -> Result<(), MeshOpError> {
            o.try_reserve(s.len()).map_err(|_| MeshOpError::AllocationFailed)?;
            o.extend_from_slice(s);
            Ok(())
        };
        put(&mut o, b"{\"determinism\":\"")?;
        put(&mut o, self.determinism.as_str().as_bytes())?;
        put(&mut o, b"\",\"inputs\":[")?;
        for (i, d) in self.inputs.iter().enumerate() {
            if i > 0 {
                put(&mut o, b",")?;
            }
            put(&mut o, format!("\"{}\"", d.as_str()).as_bytes())?;
        }
        put(&mut o, format!("],\"kind\":\"{RECORD_KIND}\",\"op\":\"{}\"", self.op.as_str()).as_bytes())?;
        put(&mut o, format!(",\"output\":\"{}\",\"params\":", self.output.as_str()).as_bytes())?;
        put(&mut o, &self.params_json)?;
        put(&mut o, b",\"quality_after\":")?;
        put(&mut o, &self.quality_after.canonical_json()?)?;
        put(&mut o, b",\"quality_before\":[")?;
        for (i, q) in self.quality_before.iter().enumerate() {
            if i > 0 {
                put(&mut o, b",")?;
            }
            put(&mut o, &q.canonical_json()?)?;
        }
        let seed = match self.seed {
            Some(s) => format!("\"{s}\""),
            None => "null".to_string(),
        };
        put(
            &mut o,
            format!(
                "],\"seed\":{seed},\"synthesized\":\"{}\",\"triangles\":\"{}\"}}",
                self.flags.count(),
                self.mesh.triangle_count()
            )
            .as_bytes(),
        )?;
        Ok(o)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    /// Registration accepts exactly D0 and D2 (REQ-002), hand-labelled.
    #[test]
    fn determinism_classes() {
        for (d, ok) in [
            (Determinism::D0, true),
            (Determinism::D1, false),
            (Determinism::D2, true),
            (Determinism::D3, false),
            (Determinism::N, false),
        ] {
            assert_eq!(check_determinism(d).is_ok(), ok, "{d}");
        }
        assert_eq!(Determinism::ALL.len(), 5, "a new class must be classified here");
    }
}
