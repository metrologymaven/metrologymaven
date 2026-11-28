//! Synthetic rounded, chamfered and burred edge fixtures (spec node `SCAN.EDGE.EST.EFIX`).
//!
//! Test support behind the `testkit` feature: one seeded generator, [`edge_fixture`], so that
//! every edge-estimation method (`SCAN.EDGE.EST.TANG`, `SCAN.EDGE.EST.SILH`) is tested against
//! the same data and the same analytic truth. Everything it returns is **synthetic**: the
//! clouds and meshes are generated, never measured, and the manifests say so
//! (`"data":"synthetic"`, D-045).
//!
//! # Geometry (pinned)
//!
//! A station has origin `p` on the nominal edge, unit tangent `t` along the edge, `n` the
//! nominal top-face normal and `b` the outward direction in the top-face plane, with
//! `b x t = n`. Station coordinates are `(u, v)`: `u` along `b`, `v` along `n`, in metres.
//! The sheet occupies `v` in `[-T, 0]`; the actual edge face (the trim line) is at `u = e`, the
//! trim offset. The profile starts on the top face at `u = e - top_extent`, crosses the edge
//! and, for wrapped coverage, runs back along the bottom face to `u = e - bottom_extent`:
//!
//! * sharp: top face to `(e, 0)`, edge face to `(e, -T)`, bottom face;
//! * round `r` (`0 < r <= T`): top face to `(e - r, 0)`, a quarter arc with centre
//!   `(e - r, -r)` to `(e, -r)` whose normal turns from `+n` to `+b`, the edge face to
//!   `(e, -T)` when `r < T`, bottom face;
//! * chamfer `c` (`0 < c <= T`, both legs `c`, so 45 degrees): top face to `(e - c, 0)`, the
//!   chamfer to `(e, -c)`, the edge face to `(e, -T)` when `c < T`, bottom face;
//! * burr `h x w` (`0 < h < T`, `w > 0`): a lip at the bottom of the edge face that stands
//!   out by `w` along `b` and is `h` high, flush with the bottom face: top face to `(e, 0)`,
//!   edge face to `(e, -(T - h))`, the ledge out to `(e + w, -(T - h))` (facing `+n`), the
//!   burr face down to `(e + w, -T)`, then the bottom face from `(e + w, -T)` inward.
//!
//! One-sided coverage is the profile before its first vertical or downward-facing segment
//! (the top face, plus the round's arc or the chamfer); wrapped coverage is all of it.
//!
//! # Sampling
//!
//! Each covered segment of length `len` gets `m = max(1, ceil(len / spacing))` equal intervals
//! (segment end points exact, junction points once), so the profile spacing never exceeds the
//! stated one. The path of length `L` gets `ceil(L / spacing)` equal intervals; the points
//! form a row-major grid, one row per path position (`grid = (rows, cols)` on the cloud).
//! Interior samples carry the analytic normal of their segment; a junction sample carries the
//! common normal of its two segments, or their normalised bisector at a crease. The mesh has
//! the same vertices, and each grid cell is split into two triangles wound so that their
//! normals point out of the material.
//!
//! The path is straight (`p(s) = (0, s, 0)`, `t = +y`, `n = +z`, `b = +x`) or a convex arc of
//! radius `R` about the origin in the `z = 0` plane (`p(s) = R (cos a, sin a, 0)` with
//! `a = s / R`, `b` radial outward, `t = (-sin a, cos a, 0)`, `n = +z`); a point `(u, v)` at
//! path position `s` is `p(s) + u b + v n`. There are `station_count` stations at
//! `s_k = L (k + 1/2) / station_count`.
//!
//! # Noise
//!
//! With `sigma > 0`, sample `i` (row-major grid index) moves by `sigma z_i` along its normal,
//! where `z_i` is a standard normal from the Box-Muller transform of draws `2i` and `2i + 1`
//! of the `fid-num` counter-based generator (seed, stream 0): `u1 = 1 - U(2i)` in `(0, 1]`,
//! `u2 = U(2i + 1)`, `z = sqrt(-2 ln u1) cos(2 pi u2)`. The normals stay analytic. With
//! `sigma = 0` nothing is drawn and the points lie on the analytic profile (REQ-001).
//! Transcendentals come from `fid_num::f` and nothing uses FMA, so the output is
//! byte-identical per seed on every platform (determinism class D2, REQ-003).
//!
//! # Truths (per station, coverage independent)
//!
//! * trim offset: `e`;
//! * tangency break for the threshold `theta` in `(0, pi)`: the first profile point at which
//!   the normal's angle to `n` exceeds `theta`; on the round's arc the point at angle `theta`,
//!   otherwise the start of the first segment whose angle exceeds `theta`; `None` if there is
//!   none;
//! * projected silhouette: the profile point with the largest `u`, the first in profile order.
//!
//! All three are in station coordinates `(u, v)` ([`StationFrame::world`] maps them to the part
//! frame).
//!
//! Every refusal is an [`EfixError`]: a spec rule names its field and the rule broken, an
//! oversized grid its point count and the limit. Nothing is returned on error.

mod profile;
mod set;

pub use set::{fixture_manifest, named_fixture, named_fixtures, NamedFixture, MANIFEST_FORMAT, NOISY_SIGMA_M};

use core::f64::consts::{PI, TAU};
use core::fmt;

use fid_geo_types::{AttributeSet, FrameRef, GeoTypeError, Mesh, PointBuffer, PointCloud};
use fid_num::rng::{Address, Cursor};
use fid_num::NumError;

/// The generator ID recorded in every manifest.
pub const GENERATOR: &str = "scan.edge.efix@1.0.0";
/// The part frame of every fixture element.
pub const EFIX_FRAME: FrameRef = FrameRef::from_bytes(*b"fid-efix-frame01");
/// The largest accepted length parameter, in metres.
pub const MAX_LENGTH_M: f64 = 1.0;
/// The largest number of grid points one fixture may hold.
pub const MAX_POINTS: u64 = 1 << 20;
/// The largest station count.
pub const MAX_STATIONS: u32 = 10_000;

/// The end profile of the sheet.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeProfile {
    /// A sharp 90-degree edge.
    Sharp,
    /// A round of radius `radius_m` between the top face and the edge face.
    Round { radius_m: f64 },
    /// A 45-degree chamfer with both legs `width_m`.
    Chamfer { width_m: f64 },
    /// A lip at the bottom of the edge face, `height_m` high and standing out by `width_m`.
    Burr { height_m: f64, width_m: f64 },
}

/// Which part of the profile is scanned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Coverage {
    /// The top face, plus the round's arc or the chamfer (seen from `+n`).
    OneSided,
    /// The whole profile: top face, edge, part of the bottom face.
    Wrapped,
}

/// The nominal edge the profile is swept along.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NominalPath {
    /// A straight edge from the origin along `+y`.
    Straight { length_m: f64 },
    /// A convex arc of radius `radius_m` about the origin, from `+x` counter-clockwise, of arc
    /// length `length_m` (at most half a turn).
    Arc { radius_m: f64, length_m: f64 },
}

impl NominalPath {
    /// The path length in metres.
    pub fn length_m(&self) -> f64 {
        match *self {
            NominalPath::Straight { length_m } | NominalPath::Arc { length_m, .. } => length_m,
        }
    }

    /// The station frame at path position `s_m`.
    pub fn frame_at(&self, s_m: f64) -> Result<StationFrame, EfixError> {
        match *self {
            NominalPath::Straight { .. } => {
                Ok(StationFrame { p: [0.0, s_m, 0.0], t: [0.0, 1.0, 0.0], n: [0.0, 0.0, 1.0], b: [1.0, 0.0, 0.0] })
            }
            NominalPath::Arc { radius_m, .. } => {
                let a = s_m / radius_m;
                let (c, s) = (fid_num::f::cos(a)?, fid_num::f::sin(a)?);
                Ok(StationFrame {
                    p: [radius_m * c, radius_m * s, 0.0],
                    t: [0.0 - s, c, 0.0],
                    n: [0.0, 0.0, 1.0],
                    b: [c, s, 0.0],
                })
            }
        }
    }
}

/// The parameters of one fixture. Validated by [`edge_fixture`]; lengths in metres, the
/// threshold in radians.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeFixtureSpec {
    pub profile: EdgeProfile,
    /// Sheet thickness `T`.
    pub thickness_m: f64,
    /// Trim offset `e`: where the actual edge face lies along `b` from the nominal edge.
    pub trim_offset_m: f64,
    pub coverage: Coverage,
    pub nominal_edge: NominalPath,
    /// The largest sample spacing, along the path and along the profile.
    pub spacing_m: f64,
    /// Standard deviation of the noise along the profile normal (0 for none).
    pub sigma_m: f64,
    /// How far inward from the trim line the top face is sampled.
    pub top_extent_m: f64,
    /// How far inward from the trim line the bottom face is sampled (wrapped coverage).
    pub bottom_extent_m: f64,
    pub station_count: u32,
    /// The tangency-break threshold angle `theta`.
    pub threshold_rad: f64,
}

/// One station: origin and unit axes in the part frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationFrame {
    pub p: [f64; 3],
    pub t: [f64; 3],
    pub n: [f64; 3],
    pub b: [f64; 3],
}

impl StationFrame {
    /// The part-frame point of station coordinates `uv`: `p + u b + v n`.
    pub fn world(&self, uv: [f64; 2]) -> [f64; 3] {
        let mut out = [0.0; 3];
        for (k, o) in out.iter_mut().enumerate() {
            *o = (self.p[k] + uv[0] * self.b[k]) + uv[1] * self.n[k];
        }
        out
    }
}

/// The analytic truth at one station (coverage independent), in station coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StationTruth {
    index: u32,
    s_m: f64,
    frame: StationFrame,
    trim_offset_m: f64,
    tangency_uv: Option<[f64; 2]>,
    silhouette_uv: [f64; 2],
}

impl StationTruth {
    /// The station index, `0..station_count`.
    pub fn index(&self) -> u32 {
        self.index
    }
    /// The path position of the station.
    pub fn s_m(&self) -> f64 {
        self.s_m
    }
    /// The station frame.
    pub fn frame(&self) -> StationFrame {
        self.frame
    }
    /// The trim offset `e` along `b`.
    pub fn trim_offset_m(&self) -> f64 {
        self.trim_offset_m
    }
    /// The tangency-break point for the spec's threshold, or `None` if the profile has none.
    pub fn tangency_uv(&self) -> Option<[f64; 2]> {
        self.tangency_uv
    }
    /// The projected-silhouette point.
    pub fn silhouette_uv(&self) -> [f64; 2] {
        self.silhouette_uv
    }
}

/// A generated fixture: the cloud (with normals), the mesh, the nominal edge and the truths.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgeFixture {
    spec: EdgeFixtureSpec,
    seed: u64,
    rows: u64,
    cols: u64,
    cloud: PointCloud,
    mesh: Mesh,
    truths: Vec<StationTruth>,
}

impl EdgeFixture {
    /// The spec it was generated from.
    pub fn spec(&self) -> &EdgeFixtureSpec {
        &self.spec
    }
    /// The noise seed.
    pub fn seed(&self) -> u64 {
        self.seed
    }
    /// The grid shape `(rows, cols)`: rows along the path, columns along the profile.
    pub fn grid(&self) -> (u64, u64) {
        (self.rows, self.cols)
    }
    /// The point cloud with unit normals (synthetic).
    pub fn cloud(&self) -> &PointCloud {
        &self.cloud
    }
    /// The triangle mesh over the same points (synthetic).
    pub fn mesh(&self) -> &Mesh {
        &self.mesh
    }
    /// The nominal edge.
    pub fn nominal_edge(&self) -> NominalPath {
        self.spec.nominal_edge
    }
    /// The per-station truths, in station order.
    pub fn truths(&self) -> &[StationTruth] {
        &self.truths
    }
}

/// Why a fixture was not generated. Nothing is returned on error.
#[derive(Debug, Clone, PartialEq)]
pub enum EfixError {
    /// A parameter is NaN or infinite.
    NonFinite { field: &'static str },
    /// A parameter breaks a range rule; `reason` names the rule.
    OutOfRange { field: &'static str, reason: &'static str },
    /// The grid would hold `needed` points (saturating), more than [`MAX_POINTS`].
    TooManyPoints { needed: u64, limit: u64 },
    /// A `fid-num` operation failed.
    Num(NumError),
    /// The point cloud or mesh was refused by `fid-geo-types`.
    Geo(GeoTypeError),
}

impl EfixError {
    /// A stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            EfixError::NonFinite { .. } => "efix.non_finite",
            EfixError::OutOfRange { .. } => "efix.out_of_range",
            EfixError::TooManyPoints { .. } => "efix.too_many_points",
            EfixError::Num(_) => "efix.num",
            EfixError::Geo(_) => "efix.geo",
        }
    }
}

impl fmt::Display for EfixError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EfixError::NonFinite { field } => write!(f, "efix.non_finite: {field} is not finite"),
            EfixError::OutOfRange { field, reason } => write!(f, "efix.out_of_range: {field}: {reason}"),
            EfixError::TooManyPoints { needed, limit } => {
                write!(f, "efix.too_many_points: {needed} grid points, limit {limit}")
            }
            EfixError::Num(e) => write!(f, "efix.num: {e}"),
            EfixError::Geo(e) => write!(f, "efix.geo: {e}"),
        }
    }
}

impl std::error::Error for EfixError {}

impl From<NumError> for EfixError {
    fn from(e: NumError) -> Self {
        EfixError::Num(e)
    }
}

impl From<GeoTypeError> for EfixError {
    fn from(e: GeoTypeError) -> Self {
        EfixError::Geo(e)
    }
}

fn finite(field: &'static str, x: f64) -> Result<f64, EfixError> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err(EfixError::NonFinite { field })
    }
}

/// A finite length in `(0, MAX_LENGTH_M]`.
fn positive_length(field: &'static str, x: f64) -> Result<f64, EfixError> {
    finite(field, x)?;
    if x <= 0.0 {
        return Err(EfixError::OutOfRange { field, reason: "must be greater than zero" });
    }
    if x > MAX_LENGTH_M {
        return Err(EfixError::OutOfRange { field, reason: "must be at most MAX_LENGTH_M" });
    }
    Ok(x)
}

fn range(field: &'static str, ok: bool, reason: &'static str) -> Result<(), EfixError> {
    if ok {
        Ok(())
    } else {
        Err(EfixError::OutOfRange { field, reason })
    }
}

/// Checks every rule of the spec, in field order; the first failure is returned.
pub fn validate(spec: &EdgeFixtureSpec) -> Result<(), EfixError> {
    let t = positive_length("thickness_m", spec.thickness_m)?;
    let e = finite("trim_offset_m", spec.trim_offset_m)?;
    range("trim_offset_m", e.abs() <= MAX_LENGTH_M, "magnitude must be at most MAX_LENGTH_M")?;
    let inward = match spec.profile {
        EdgeProfile::Sharp => 0.0,
        EdgeProfile::Round { radius_m } => {
            let r = positive_length("profile.radius_m", radius_m)?;
            range("profile.radius_m", r <= t, "must be at most the thickness")?;
            r
        }
        EdgeProfile::Chamfer { width_m } => {
            let c = positive_length("profile.width_m", width_m)?;
            range("profile.width_m", c <= t, "must be at most the thickness")?;
            c
        }
        EdgeProfile::Burr { height_m, width_m } => {
            let h = positive_length("profile.height_m", height_m)?;
            range("profile.height_m", h < t, "must be less than the thickness")?;
            positive_length("profile.width_m", width_m)?;
            0.0
        }
    };
    let top = positive_length("top_extent_m", spec.top_extent_m)?;
    range("top_extent_m", top > inward, "must exceed the inward extent of the round or chamfer")?;
    let bottom = positive_length("bottom_extent_m", spec.bottom_extent_m)?;
    match spec.nominal_edge {
        NominalPath::Straight { length_m } => {
            positive_length("nominal_edge.length_m", length_m)?;
        }
        NominalPath::Arc { radius_m, length_m } => {
            let r = positive_length("nominal_edge.radius_m", radius_m)?;
            let l = positive_length("nominal_edge.length_m", length_m)?;
            range("nominal_edge.length_m", l <= PI * r, "an arc path may sweep at most half a turn")?;
            let reach = if top > bottom { top } else { bottom };
            range("nominal_edge.radius_m", r + (e - reach) > 0.0, "the profile must stay outside the arc centre")?;
        }
    }
    positive_length("spacing_m", spec.spacing_m)?;
    let sigma = finite("sigma_m", spec.sigma_m)?;
    range("sigma_m", (0.0..=MAX_LENGTH_M).contains(&sigma), "must be in [0, MAX_LENGTH_M]")?;
    range("station_count", (1..=MAX_STATIONS).contains(&spec.station_count), "must be in 1..=MAX_STATIONS")?;
    let theta = finite("threshold_rad", spec.threshold_rad)?;
    range("threshold_rad", theta > 0.0 && theta < PI, "must be in (0, pi)")?;
    Ok(())
}

/// A float count already bounded by [`MAX_POINTS`], as `u64`.
fn count(x: f64) -> u64 {
    // In [1, MAX_POINTS] by construction, so the conversion is exact.
    x as u64
}

/// One standard normal from draws `2i` and `2i + 1` (Box-Muller, see the module docs).
fn standard_normal(seed: u64, i: u64) -> Result<f64, EfixError> {
    let mut c = Cursor::new(Address { seed, stream: 0 }, i.wrapping_mul(2));
    let u1 = 1.0 - c.uniform_f64();
    let u2 = c.uniform_f64();
    let r2 = 0.0 - 2.0 * fid_num::f::ln(u1)?;
    Ok(fid_num::f::sqrt(r2)? * fid_num::f::cos(TAU * u2)?)
}

/// Generates the fixture of `spec` with noise seed `seed` (see the module documentation).
pub fn edge_fixture(spec: &EdgeFixtureSpec, seed: u64) -> Result<EdgeFixture, EfixError> {
    validate(spec)?;
    let segs = profile::segments(spec)?;
    let covered = profile::covered(&segs, spec.coverage == Coverage::OneSided);
    // Sizes first, bounded before anything is allocated.
    let mut cols_f = 1.0;
    for seg in segs.iter().take(covered) {
        cols_f += profile::intervals(seg.length, spec.spacing_m);
    }
    let length = spec.nominal_edge.length_m();
    let along = profile::intervals(length, spec.spacing_m);
    let rows_f = along + 1.0;
    let limit = MAX_POINTS as f64;
    if rows_f * cols_f > limit {
        let needed = rows_f * cols_f;
        let needed = if needed < u64::MAX as f64 { needed as u64 } else { u64::MAX };
        return Err(EfixError::TooManyPoints { needed, limit: MAX_POINTS });
    }
    let (rows, cols) = (count(rows_f), count(cols_f));
    let profile_pts = profile::samples(&segs, covered, spec.spacing_m)?;
    if profile_pts.len() as u64 != cols {
        return Err(EfixError::OutOfRange {
            field: "profile",
            reason: "sample count differs from the bounded grid size",
        });
    }
    let n = (rows * cols) as usize;
    let mut xyz: [Vec<f64>; 3] = [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    let mut nrm: [Vec<f64>; 3] = [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    let mut i: u64 = 0;
    for row in 0..rows {
        let s = if row + 1 == rows { length } else { length * ((row as f64) / along) };
        let fr = spec.nominal_edge.frame_at(s)?;
        for &(uv, nuv) in &profile_pts {
            let mut q = fr.world(uv);
            let nw = [
                nuv[0] * fr.b[0] + nuv[1] * fr.n[0],
                nuv[0] * fr.b[1] + nuv[1] * fr.n[1],
                nuv[0] * fr.b[2] + nuv[1] * fr.n[2],
            ];
            if spec.sigma_m > 0.0 {
                let d = spec.sigma_m * standard_normal(seed, i)?;
                for (qk, nk) in q.iter_mut().zip(nw) {
                    *qk += d * nk;
                }
            }
            for ((col, ncol), (qk, nk)) in xyz.iter_mut().zip(nrm.iter_mut()).zip(q.into_iter().zip(nw)) {
                col.push(qk);
                ncol.push(nk);
            }
            i += 1;
        }
    }
    let [x, y, z] = xyz;
    let buf = PointBuffer::new(x, y, z);
    let attrs = AttributeSet { normal: Some(nrm), grid: Some((rows, cols)), ..AttributeSet::default() };
    let cloud = PointCloud::new(EFIX_FRAME, buf.clone(), attrs)?;
    let (cells_r, cells_c) = (rows.saturating_sub(1), cols.saturating_sub(1));
    let mut tris = Vec::with_capacity(2 * (cells_r * cells_c) as usize);
    for r in 0..cells_r {
        for c in 0..cells_c {
            let a = r * cols + c;
            let (b, cc, d) = (a + 1, a + cols + 1, a + cols);
            tris.push([a, b, cc]);
            tris.push([a, cc, d]);
        }
    }
    let mesh = Mesh::new(EFIX_FRAME, buf, AttributeSet::default(), tris)?;
    let tangency = profile::tangency(&segs, spec.threshold_rad)?;
    let silhouette = profile::silhouette(&segs);
    let mut truths = Vec::with_capacity(spec.station_count as usize);
    let count_f = f64::from(spec.station_count);
    for k in 0..spec.station_count {
        let s_m = length * (f64::from(k) + 0.5) / count_f;
        truths.push(StationTruth {
            index: k,
            s_m,
            frame: spec.nominal_edge.frame_at(s_m)?,
            trim_offset_m: spec.trim_offset_m,
            tangency_uv: tangency,
            silhouette_uv: silhouette,
        });
    }
    Ok(EdgeFixture { spec: *spec, seed, rows, cols, cloud, mesh, truths })
}
