//! The registration report and the registered-version writer. Spec node
//! `SCAN.CLOUD.REG.RREP`.
//!
//! Every registration command ends here: it builds a [`RegistrationReport`] from its input
//! clouds, the computed poses and its solver summary, then calls [`write_registered`], which
//! maps each input into the reference frame by GEO.FRAME.APPLY `transform_element` and attaches
//! a [`RegisteredMarker`] (group ID, report digest, pose, reference frame). Downstream steps
//! test that marker instead of assuming that clouds share a frame. Storing the new versions and
//! journaling them is CORE's business; this module computes them and never touches an input.
//!
//! # Poses
//!
//! A pose maps the coordinates of one input cloud (in the inputs' shared frame) into the
//! reference frame. Poses are [`Transform`]s so a non-rigid one can be refused by name: a
//! [`Transform::Similarity`] is `geo.frame.not_rigid` whatever its scale
//! ([`RegError::NotRigid`]), and [`rigid_from_matrix4`] turns a 4 x 4 matrix into a [`Rigid`]
//! only when its last row is exactly `0 0 0 1` and its rotation block passes GEO.FRAME.XFORM's
//! checks (finite, orthonormal within 1e-12, determinant within 1e-12 of +1).
//!
//! # Report construction (first failure wins)
//!
//! [`RegistrationReport::new`]`(command, inputs, poses, reference, solver)`:
//!
//! 1. The command ID is in canonical spelling, `name@major` with no leading zero in the major
//!    (`command`), so one command has one group ID.
//! 2. `2 <=` N `<=` [`MAX_SCANS`] inputs (`inputs`) and exactly N poses (`pose`).
//! 3. Every pose is rigid, in index order ([`RegError::NotRigid`] naming the scan).
//! 4. Every input, in index order, is a non-empty point cloud (`inputs`) in the frame of input 0
//!    ([`RegError::Scan`] wrapping `geo.frame.mismatch`); its version digest is its
//!    GEO.TYPES.DIGEST content digest, computed here, never taken from the caller.
//! 5. No two inputs have the same version digest (`inputs`; the same cloud twice).
//! 6. Per scan, the translation is `sqrt(dot3(t, t))` (m) and the rotation angle
//!    `acos(clamp(((R00 + R11) + R22 - 1) / 2, -1, 1))` (rad), with `dot3(a, b) = (a0 * b0 +
//!    a1 * b1) + a2 * b2`, the correctly rounded `fid_num::f::sqrt`, `fid_num::f::total_clamp`
//!    and `fid_num::f::acos` (the pinned libm port; faithfully rounded, bit-identical on every
//!    platform). A translation whose square overflows is refused (`pose`).
//! 7. The verdict is terminal (`verdict`): the status is `converged` for an
//!    `ALN.BFIT.STOP` `converged(..)` verdict and `not_converged` for a `stopped(..)` one, and
//!    the stop reason is the verdict's text, so status and reason can never disagree.
//! 8. The global RMS residual is finite and `>= 0` (`global_rms_m`).
//! 9. At least one pair statistic (`pairs`); every pair names two scans below N (`pairs`); the
//!    pairs are sorted by `(a, b)` and a repeated `(a, b)` is refused (`pairs`); every scan
//!    appears in at least one pair (`pairs`), so no pose is reported without a residual behind
//!    it. [`PairStat::new`] has already refused `a == b`, an overlap outside `[0, 1]`, a
//!    negative or non-finite RMS or maximum, and a pair with no residual samples.
//!
//! Each refusal is a [`RegError`]; nothing is built. `-0.0` in a pair statistic or the global
//! RMS is stored as `+0.0`. RMS `<=` maximum is not enforced: a correctly computed RMS can sit a
//! few ulps above the maximum.
//!
//! # Canonical JSON, schema `fid.scan.registration-report/1` (REQ-003, REQ-004)
//!
//! Kernel canonical JSON (`fid-kci`: RFC 8785 restricted to strings, arrays and objects, keys
//! sorted by UTF-16 code units, no whitespace). By kernel convention every number is a string:
//! a count or an index is its decimal form; an `f64` is `"0x"` and the 16 lowercase hex digits
//! of its IEEE 754 bits, with `-0.0` written as `+0.0`; a quantity is `{"unit": u, "value":
//! hex}` with `u` = `"m"`, `"rad"` or `"1"` (dimensionless). Every member is required:
//!
//! ```text
//! { "algorithm_id": AlgorithmId text, "command": command ID,
//!   "global_rms": {m},
//!   "pairs": [ { "a", "b", "max_abs": {m}, "n", "overlap": {1}, "rms": {m} } ... ]  (a, b) ascending,
//!   "reference_frame": 32 lowercase hex digits of the frame ID,
//!   "scans": [ { "rotation": {rad}, "scan", "transform": [16 hex, row-major 4 x 4, translation
//!                entries 3, 7, 11 in metres, last row 0 0 0 1], "translation": {m},
//!                "version_digest": "sha256:" + 64 hex } ... ]  in input order,
//!   "schema": "fid.scan.registration-report/1",
//!   "status": "converged" | "not_converged", "stop_reason": the ALN.BFIT.STOP verdict text }
//! ```
//!
//! The report digest is SHA-256 of these bytes (`fid-kci`), in the shared `sha256:` form.
//!
//! # Group ID (REQ-005)
//!
//! UUIDv5 in [`GROUP_NAMESPACE`] of `command ID bytes | 0x00 | the N version digests (32 raw
//! bytes each) in ascending byte order`. The command ID holds no `0x00` and every digest has a
//! fixed length, so the name is unambiguous; sorting makes the ID independent of input order.
//!
//! # Writing (REQ-001, REQ-002)
//!
//! [`write_registered`]`(inputs, poses, report, reference)` checks, first failure wins: the
//! reference frame is the report's (`reference`); N inputs and N poses (`inputs`, `pose`);
//! every pose rigid ([`RegError::NotRigid`]); every pose equal, bit for bit after `-0.0` to
//! `+0.0`, to the report's (`report`); every input a point cloud (`inputs`) whose digest is the
//! report's (`report`). Then each input in index order becomes `transform_element(input,
//! Transform::Rigid(pose), reference)` (a failure is [`RegError::Transform`]). Only when every
//! scan succeeded are the new versions returned, one per input (a scan whose pose is the
//! identity gets one too, so every member of the group is marked), each with its input's
//! digest and a marker holding the group ID, the report digest, the pose and the reference
//! frame. Inputs are only read.
//!
//! KNP-1: no fused multiply-add, no `f64::min`/`f64::max`, no hash containers; floats are
//! compared only in ways that refuse NaN, the clamp is `fid_num::f::total_clamp`, and nothing
//! here panics.

use fid_align_solve::Verdict;
use fid_geo_frame::{require_same_frame_ref, transform_element, Matrix3, Rigid, Transform};
use fid_geo_types::digest::canonical_f64_bits;
use fid_geo_types::{digest, ElementDigest, FrameRef, GeometryElement, PointCloud};
use fid_kci::kci::json::{to_canonical, Json};
use fid_kci::kci::sha256_digest;
use fid_kci::AlgorithmId;
use fid_types::{CommandId, Digest};

use super::error::RegError;
use super::graph::MAX_SCANS;
use super::uuid::{format_uuid, uuid_v5};

/// The schema name and version written into every report.
pub const REPORT_SCHEMA: &str = "fid.scan.registration-report/1";

/// The namespace of registration group IDs: UUIDv5 of
/// `https://github.com/metrologymaven/metrologymaven/spec/scan/cloud/reg/rrep#registration-group-v1`
/// in the RFC 9562 URL namespace, i.e. `4e6b1254-8fba-5cf6-9e18-444c12774578` (checked by the
/// fixture test against Python's `uuid.uuid5`).
pub const GROUP_NAMESPACE: [u8; 16] =
    [0x4e, 0x6b, 0x12, 0x54, 0x8f, 0xba, 0x5c, 0xf6, 0x9e, 0x18, 0x44, 0x4c, 0x12, 0x77, 0x45, 0x78];

/// The convergence status of the registration solver (REQ-003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegStatus {
    /// The solver converged. Wire `converged`.
    Converged,
    /// The solver stopped without converging (iteration limit or divergence). Wire
    /// `not_converged`.
    NotConverged,
}

impl RegStatus {
    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            RegStatus::Converged => "converged",
            RegStatus::NotConverged => "not_converged",
        }
    }
}

/// `-0.0` as `+0.0`; every other value unchanged.
fn plus_zero(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

/// The hex form of an `f64`: `0x` and 16 lowercase hex digits of its canonical bits.
fn hex64(v: f64) -> String {
    format!("0x{:016x}", canonical_f64_bits(v))
}

fn s(v: &str) -> Json {
    Json::Str(v.to_string())
}

fn quantity(unit: &str, v: f64) -> Json {
    Json::Obj(vec![("unit".to_string(), s(unit)), ("value".to_string(), Json::Str(hex64(v)))])
}

fn index_u32(i: usize) -> Result<u32, RegError> {
    u32::try_from(i).map_err(|_| RegError::Inconsistent)
}

/// The statistics of one ordered scan pair: scan `a` (moving) against scan `b` (target), as
/// the registration command computed them at the final poses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairStat {
    a: u32,
    b: u32,
    overlap: f64,
    rms_m: f64,
    max_m: f64,
    n: u64,
}

impl PairStat {
    /// A validated pair statistic: `a != b`; `overlap` finite in `[0, 1]`; `rms_m` and
    /// `max_m` finite and `>= 0` (metres); `n >= 1` residual samples. Each refusal is
    /// `scan.cloud.invalid_parameter` (`pairs`). `-0.0` is stored as `+0.0`.
    pub fn new(a: u32, b: u32, overlap: f64, rms_m: f64, max_m: f64, n: u64) -> Result<PairStat, RegError> {
        let bad = |reason: String| RegError::invalid("pairs", reason);
        if a == b {
            return Err(bad(format!("pair ({a}, {b}) names one scan twice")));
        }
        if !(overlap.is_finite() && (0.0..=1.0).contains(&overlap)) {
            return Err(bad(format!("pair ({a}, {b}): overlap {overlap} is not in [0, 1]")));
        }
        for (what, v) in [("rms", rms_m), ("max", max_m)] {
            if !(v.is_finite() && v >= 0.0) {
                return Err(bad(format!("pair ({a}, {b}): {what} residual {v} m is not finite and >= 0")));
            }
        }
        if n == 0 {
            return Err(bad(format!("pair ({a}, {b}) has no residual samples, so no RMS or maximum")));
        }
        Ok(PairStat { a, b, overlap: plus_zero(overlap), rms_m: plus_zero(rms_m), max_m: plus_zero(max_m), n })
    }

    /// The moving scan index.
    pub fn a(&self) -> u32 {
        self.a
    }

    /// The target scan index.
    pub fn b(&self) -> u32 {
        self.b
    }

    /// The overlap fraction of `a` against `b` (dimensionless, in `[0, 1]`).
    pub fn overlap(&self) -> f64 {
        self.overlap
    }

    /// The RMS residual in metres.
    pub fn rms_m(&self) -> f64 {
        self.rms_m
    }

    /// The largest absolute residual in metres.
    pub fn max_m(&self) -> f64 {
        self.max_m
    }

    /// The number of residual samples.
    pub fn n(&self) -> u64 {
        self.n
    }

    fn json(&self) -> Json {
        Json::Obj(vec![
            ("a".to_string(), Json::Str(self.a.to_string())),
            ("b".to_string(), Json::Str(self.b.to_string())),
            ("max_abs".to_string(), quantity("m", self.max_m)),
            ("n".to_string(), Json::Str(self.n.to_string())),
            ("overlap".to_string(), quantity("1", self.overlap)),
            ("rms".to_string(), quantity("m", self.rms_m)),
        ])
    }
}

/// What the registration solver reports: its algorithm ID, its terminal stop verdict, the
/// per-pair statistics and the global RMS residual. Plain input data; [`RegistrationReport::new`]
/// validates it.
#[derive(Debug, Clone, PartialEq)]
pub struct SolverSummary {
    /// The solver's algorithm ID (for example `scan.reg.global_p2plane@1.0.0`).
    pub algorithm: AlgorithmId,
    /// The terminal `ALN.BFIT.STOP` verdict (never `continue`).
    pub verdict: Verdict,
    /// One statistic per ordered pair the solver used, in any order.
    pub pairs: Vec<PairStat>,
    /// The global RMS residual in metres.
    pub global_rms_m: f64,
}

/// One scan of a report: its index, input version digest, pose and the pose's translation
/// length and rotation angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScanPose {
    scan: u32,
    version_digest: ElementDigest,
    pose: Rigid,
    translation_m: f64,
    rotation_rad: f64,
}

impl ScanPose {
    /// The scan index (input order).
    pub fn scan(&self) -> u32 {
        self.scan
    }

    /// The GEO.TYPES.DIGEST content digest of the input cloud version.
    pub fn version_digest(&self) -> ElementDigest {
        self.version_digest
    }

    /// The pose (input frame to reference frame).
    pub fn pose(&self) -> Rigid {
        self.pose
    }

    /// The pose as a row-major 4 x 4 matrix with last row `0 0 0 1`.
    pub fn transform(&self) -> [[f64; 4]; 4] {
        let r = self.pose.r();
        let t = self.pose.t();
        [
            [r[0][0], r[0][1], r[0][2], t[0]],
            [r[1][0], r[1][1], r[1][2], t[1]],
            [r[2][0], r[2][1], r[2][2], t[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// The length of the translation in metres.
    pub fn translation_m(&self) -> f64 {
        self.translation_m
    }

    /// The rotation angle in radians, in `[0, pi]`.
    pub fn rotation_rad(&self) -> f64 {
        self.rotation_rad
    }

    fn json(&self) -> Json {
        let transform = self.transform().iter().flat_map(|row| row.iter().map(|&v| Json::Str(hex64(v)))).collect();
        Json::Obj(vec![
            ("rotation".to_string(), quantity("rad", self.rotation_rad)),
            ("scan".to_string(), Json::Str(self.scan.to_string())),
            ("transform".to_string(), Json::Arr(transform)),
            ("translation".to_string(), quantity("m", self.translation_m)),
            ("version_digest".to_string(), Json::Str(self.version_digest.to_digest().as_str().to_string())),
        ])
    }
}

/// The report of one registration run (REQ-003): per scan the pose, its translation and
/// rotation; per ordered pair the overlap and residuals; the global RMS residual; the solver's
/// algorithm ID, status and stop reason. Immutable; only [`RegistrationReport::new`] makes one,
/// and its canonical JSON, digest and group ID are fixed at construction.
#[derive(Debug, Clone, PartialEq)]
pub struct RegistrationReport {
    command: CommandId,
    algorithm: AlgorithmId,
    reference_frame: FrameRef,
    scans: Vec<ScanPose>,
    pairs: Vec<PairStat>,
    global_rms_m: f64,
    verdict: Verdict,
    status: RegStatus,
    json: Vec<u8>,
    digest: Digest,
    group_id: [u8; 16],
}

/// A pose as a [`Rigid`], or `geo.frame.not_rigid` naming the scan.
fn rigid_pose(scan: Option<u32>, t: &Transform) -> Result<Rigid, RegError> {
    match t {
        Transform::Rigid(r) => Ok(*r),
        Transform::Similarity(sim) => {
            Err(RegError::NotRigid { scan, reason: format!("a similarity transform (scale {})", sim.s()) })
        }
    }
}

/// `x ↦ R x + t` from a row-major 4 x 4 matrix: the last row must be exactly `0 0 0 1` and the
/// rotation block must pass GEO.FRAME.XFORM's rigid checks; anything else is
/// `geo.frame.not_rigid` ([`RegError::NotRigid`] with no scan).
pub fn rigid_from_matrix4(m: &[[f64; 4]; 4]) -> Result<Rigid, RegError> {
    let last = m[3];
    if !(last[0] == 0.0 && last[1] == 0.0 && last[2] == 0.0 && last[3] == 1.0) {
        return Err(RegError::NotRigid {
            scan: None,
            reason: format!("last row is {last:?}, not 0 0 0 1 (a projective or affine matrix)"),
        });
    }
    let r: Matrix3 = [[m[0][0], m[0][1], m[0][2]], [m[1][0], m[1][1], m[1][2]], [m[2][0], m[2][1], m[2][2]]];
    Rigid::new(r, [m[0][3], m[1][3], m[2][3]]).map_err(|e| RegError::NotRigid { scan: None, reason: e.to_string() })
}

/// The input as a point cloud, or `scan.cloud.invalid_parameter` naming `what`.
fn as_cloud<'a>(i: usize, e: &'a GeometryElement, what: &'static str) -> Result<&'a PointCloud, RegError> {
    match e {
        GeometryElement::PointCloud(p) => Ok(p),
        other => Err(RegError::invalid(what, format!("input {i} is a {}, not a point cloud", other.kind()))),
    }
}

fn translation_length(i: u32, t: [f64; 3]) -> Result<f64, RegError> {
    let sq = (t[0] * t[0] + t[1] * t[1]) + t[2] * t[2];
    if !sq.is_finite() {
        return Err(RegError::invalid("pose", format!("scan {i}: the squared translation length overflows")));
    }
    fid_num::f::sqrt(sq).map_err(|_| RegError::Inconsistent)
}

fn rotation_angle(r: &Matrix3) -> Result<f64, RegError> {
    let trace = (r[0][0] + r[1][1]) + r[2][2];
    let c = (trace - 1.0) / 2.0;
    let c = fid_num::f::total_clamp(c, -1.0, 1.0).map_err(|_| RegError::Inconsistent)?;
    fid_num::f::acos(c).map_err(|_| RegError::Inconsistent)
}

/// Whether two poses are equal bit for bit after `-0.0` to `+0.0`.
fn same_pose(a: &Rigid, b: &Rigid) -> bool {
    let (ra, rb) = (a.r(), b.r());
    let (ta, tb) = (a.t(), b.t());
    let same = |x: &f64, y: &f64| canonical_f64_bits(*x) == canonical_f64_bits(*y);
    ra.iter().flatten().zip(rb.iter().flatten()).all(|(x, y)| same(x, y))
        && ta.iter().zip(tb.iter()).all(|(x, y)| same(x, y))
}

impl RegistrationReport {
    /// Builds and validates the report of one registration run; see the
    /// [module documentation](self) for the pinned checks, their order, and the JSON layout.
    pub fn new(
        command: &CommandId,
        inputs: &[GeometryElement],
        poses: &[Transform],
        reference: FrameRef,
        solver: SolverSummary,
    ) -> Result<RegistrationReport, RegError> {
        // 1. Canonical command spelling.
        if command.as_str() != format!("{}@{}", command.name(), command.major()) {
            return Err(RegError::invalid(
                "command",
                format!(
                    "command ID {:?} is not in canonical spelling (no leading zero in the major)",
                    command.as_str()
                ),
            ));
        }
        // 2. Counts.
        let n = inputs.len();
        if !(2..=MAX_SCANS).contains(&n) {
            return Err(RegError::invalid(
                "inputs",
                format!("{n} scans; a registration report needs 2 to {MAX_SCANS}"),
            ));
        }
        if poses.len() != n {
            return Err(RegError::invalid("pose", format!("{} poses for {n} scans", poses.len())));
        }
        // 3. Rigid poses.
        let mut rigid = Vec::with_capacity(n);
        for (i, p) in poses.iter().enumerate() {
            rigid.push(rigid_pose(Some(index_u32(i)?), p)?);
        }
        // 4. Inputs: non-empty clouds in one frame; their content digests.
        let mut digests = Vec::with_capacity(n);
        let mut frame0 = None;
        for (i, e) in inputs.iter().enumerate() {
            let cloud = as_cloud(i, e, "inputs")?;
            if cloud.is_empty() {
                return Err(RegError::invalid("inputs", format!("input {i} is an empty cloud")));
            }
            let f0 = *frame0.get_or_insert(cloud.frame());
            require_same_frame_ref(f0, cloud.frame()).map_err(|g| RegError::Scan {
                index: u32::try_from(i).unwrap_or(u32::MAX),
                error: Box::new(RegError::FrameMismatch(g)),
            })?;
            digests.push(digest(e));
        }
        // 5. No cloud version twice.
        let mut sorted: Vec<(ElementDigest, usize)> = digests.iter().copied().zip(0..).collect();
        sorted.sort();
        for w in sorted.windows(2) {
            if let [(da, ia), (db, ib)] = w {
                if da == db {
                    return Err(RegError::invalid(
                        "inputs",
                        format!("inputs {ia} and {ib} are the same cloud version"),
                    ));
                }
            }
        }
        // 6. Per-scan translation and rotation.
        let mut scans = Vec::with_capacity(n);
        for (i, (pose, version_digest)) in rigid.iter().zip(digests.iter()).enumerate() {
            let scan = index_u32(i)?;
            scans.push(ScanPose {
                scan,
                version_digest: *version_digest,
                pose: *pose,
                translation_m: translation_length(scan, pose.t())?,
                rotation_rad: rotation_angle(&pose.r())?,
            });
        }
        // 7. Terminal verdict.
        let status = match solver.verdict {
            Verdict::Continue => {
                return Err(RegError::invalid("verdict", "the verdict `continue` is not terminal".to_string()))
            }
            Verdict::Converged(_) => RegStatus::Converged,
            Verdict::Stopped(_) => RegStatus::NotConverged,
        };
        // 8. Global RMS.
        let g = solver.global_rms_m;
        if !(g.is_finite() && g >= 0.0) {
            return Err(RegError::invalid("global_rms_m", format!("global RMS {g} m is not finite and >= 0")));
        }
        // 9. Pairs.
        let mut pairs = solver.pairs;
        if pairs.is_empty() {
            return Err(RegError::invalid("pairs", "no pair statistic; every registration relates a pair".to_string()));
        }
        let n32 = index_u32(n)?;
        for p in &pairs {
            if p.a >= n32 || p.b >= n32 {
                return Err(RegError::invalid("pairs", format!("pair ({}, {}) names a scan >= {n}", p.a, p.b)));
            }
        }
        pairs.sort_by_key(|x| (x.a, x.b));
        for w in pairs.windows(2) {
            if let [x, y] = w {
                if (x.a, x.b) == (y.a, y.b) {
                    return Err(RegError::invalid("pairs", format!("pair ({}, {}) is given twice", x.a, x.b)));
                }
            }
        }
        let mut covered = vec![false; n];
        for p in &pairs {
            for k in [p.a, p.b] {
                if let Some(c) = usize::try_from(k).ok().and_then(|k| covered.get_mut(k)) {
                    *c = true;
                }
            }
        }
        if let Some(i) = covered.iter().position(|c| !c) {
            return Err(RegError::invalid("pairs", format!("scan {i} appears in no pair statistic")));
        }
        // JSON, digest, group ID.
        let mut report = RegistrationReport {
            command: command.clone(),
            algorithm: solver.algorithm,
            reference_frame: reference,
            scans,
            pairs,
            global_rms_m: plus_zero(g),
            verdict: solver.verdict,
            status,
            json: Vec::new(),
            digest: Digest::TOMBSTONE,
            group_id: [0; 16],
        };
        report.json = to_canonical(&report.to_json()).map_err(|_| RegError::Inconsistent)?;
        report.digest = sha256_digest(&report.json);
        let mut name = Vec::with_capacity(command.as_str().len() + 1 + 32 * n);
        name.extend_from_slice(command.as_str().as_bytes());
        name.push(0);
        for (d, _) in &sorted {
            name.extend_from_slice(&d.to_bytes());
        }
        report.group_id = uuid_v5(&GROUP_NAMESPACE, &name);
        Ok(report)
    }

    fn to_json(&self) -> Json {
        let frame: String = self.reference_frame.to_bytes().iter().map(|b| format!("{b:02x}")).collect();
        Json::Obj(vec![
            ("algorithm_id".to_string(), Json::Str(self.algorithm.to_string())),
            ("command".to_string(), s(self.command.as_str())),
            ("global_rms".to_string(), quantity("m", self.global_rms_m)),
            ("pairs".to_string(), Json::Arr(self.pairs.iter().map(PairStat::json).collect())),
            ("reference_frame".to_string(), Json::Str(frame)),
            ("scans".to_string(), Json::Arr(self.scans.iter().map(ScanPose::json).collect())),
            ("schema".to_string(), s(REPORT_SCHEMA)),
            ("status".to_string(), s(self.status.as_str())),
            ("stop_reason".to_string(), s(self.verdict.as_str())),
        ])
    }

    /// The command that ran.
    pub fn command(&self) -> &CommandId {
        &self.command
    }

    /// The solver's algorithm ID.
    pub fn algorithm(&self) -> &AlgorithmId {
        &self.algorithm
    }

    /// The reference frame every registered version is expressed in.
    pub fn reference_frame(&self) -> FrameRef {
        self.reference_frame
    }

    /// The scans, in input order.
    pub fn scans(&self) -> &[ScanPose] {
        &self.scans
    }

    /// The pair statistics, in ascending `(a, b)` order.
    pub fn pairs(&self) -> &[PairStat] {
        &self.pairs
    }

    /// The global RMS residual in metres.
    pub fn global_rms_m(&self) -> f64 {
        self.global_rms_m
    }

    /// The convergence status.
    pub fn status(&self) -> RegStatus {
        self.status
    }

    /// The stop reason (the terminal `ALN.BFIT.STOP` verdict).
    pub fn stop_reason(&self) -> Verdict {
        self.verdict
    }

    /// The canonical JSON bytes (REQ-004).
    pub fn canonical_json(&self) -> &[u8] {
        &self.json
    }

    /// SHA-256 of [`RegistrationReport::canonical_json`]: the value every marker stores.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    /// The group ID (UUIDv5, REQ-005) as 16 bytes.
    pub fn group_id(&self) -> [u8; 16] {
        self.group_id
    }

    /// The group ID in hyphenated lowercase form.
    pub fn group_id_string(&self) -> String {
        format_uuid(&self.group_id)
    }
}

/// The metadata every registered cloud version carries (stored as element metadata by CORE).
#[derive(Debug, Clone, PartialEq)]
pub struct RegisteredMarker {
    group_id: [u8; 16],
    report_digest: Digest,
    pose: Rigid,
    reference_frame: FrameRef,
}

impl RegisteredMarker {
    /// The registration group ID (equal on every member of one registration).
    pub fn group_id(&self) -> [u8; 16] {
        self.group_id
    }

    /// The group ID in hyphenated lowercase form.
    pub fn group_id_string(&self) -> String {
        format_uuid(&self.group_id)
    }

    /// The digest of the registration report.
    pub fn report_digest(&self) -> &Digest {
        &self.report_digest
    }

    /// The pose that moved the input into the reference frame.
    pub fn pose(&self) -> Rigid {
        self.pose
    }

    /// The reference frame.
    pub fn reference_frame(&self) -> FrameRef {
        self.reference_frame
    }
}

/// One new registered cloud version: the transformed cloud, the digest of the input version it
/// came from, and its marker.
#[derive(Debug, Clone, PartialEq)]
pub struct NewCloudVersion {
    cloud: PointCloud,
    input_digest: ElementDigest,
    marker: RegisteredMarker,
}

impl NewCloudVersion {
    /// The registered cloud, in the reference frame.
    pub fn cloud(&self) -> &PointCloud {
        &self.cloud
    }

    /// The content digest of the input version (REQ-001).
    pub fn input_digest(&self) -> ElementDigest {
        self.input_digest
    }

    /// The registration marker.
    pub fn marker(&self) -> &RegisteredMarker {
        &self.marker
    }

    /// The parts: cloud, input digest, marker.
    pub fn into_parts(self) -> (PointCloud, ElementDigest, RegisteredMarker) {
        (self.cloud, self.input_digest, self.marker)
    }
}

/// One new registered version per input, in input order, each the input mapped by its pose
/// into `reference` and marked; see the [module documentation](self) for the checks and
/// their order. On any error nothing is returned, and the inputs are never modified.
pub fn write_registered(
    inputs: &[GeometryElement],
    poses: &[Transform],
    report: &RegistrationReport,
    reference: FrameRef,
) -> Result<Vec<NewCloudVersion>, RegError> {
    if reference != report.reference_frame {
        return Err(RegError::invalid("reference", "the reference frame is not the report's".to_string()));
    }
    let n = report.scans.len();
    if inputs.len() != n {
        return Err(RegError::invalid("inputs", format!("{} inputs for a report of {n} scans", inputs.len())));
    }
    if poses.len() != n {
        return Err(RegError::invalid("pose", format!("{} poses for a report of {n} scans", poses.len())));
    }
    let mut rigid = Vec::with_capacity(n);
    for (i, p) in poses.iter().enumerate() {
        rigid.push(rigid_pose(Some(index_u32(i)?), p)?);
    }
    for (i, (pose, sp)) in rigid.iter().zip(&report.scans).enumerate() {
        if !same_pose(pose, &sp.pose) {
            return Err(RegError::invalid("report", format!("the pose of scan {i} is not the report's")));
        }
    }
    for (i, (e, sp)) in inputs.iter().zip(&report.scans).enumerate() {
        as_cloud(i, e, "inputs")?;
        if digest(e) != sp.version_digest {
            return Err(RegError::invalid("report", format!("input {i} is not the cloud version the report names")));
        }
    }
    let mut out = Vec::with_capacity(n);
    for (i, (e, pose)) in inputs.iter().zip(&rigid).enumerate() {
        let index = index_u32(i)?;
        let moved = transform_element(e, &Transform::Rigid(*pose), reference)
            .map_err(|error| RegError::Transform { index, error })?;
        let cloud = match moved {
            GeometryElement::PointCloud(c) => c,
            _ => return Err(RegError::Inconsistent),
        };
        let input_digest = report.scans.get(i).map(|sp| sp.version_digest).ok_or(RegError::Inconsistent)?;
        out.push(NewCloudVersion {
            cloud,
            input_digest,
            marker: RegisteredMarker {
                group_id: report.group_id,
                report_digest: report.digest,
                pose: *pose,
                reference_frame: reference,
            },
        });
    }
    Ok(out)
}
