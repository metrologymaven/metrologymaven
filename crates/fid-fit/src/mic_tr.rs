//! Trust region of the maximum inscribed fits, the `unbounded_envelope` check and the MIC
//! defined-primitives table. Spec node `KRN.ENV.MIC.TR`.
//!
//! A largest-empty-ball problem has no bounded answer when the points do not surround the
//! centre (a partial arc, a sparse scan): the centre runs away. The MIC fits
//! (`KRN.ENV.MIC.CIRSPH`, `KRN.ENV.MIC.CYL`) therefore hold the centre, or the axis point, in a
//! [`TrustRegion`] around the least-squares solution, solve each linearised step as an LP with
//! the region's fixed linear [`TrustRows`], and hand the final centre to
//! [`TrustRegion::check_boundary`]: a centre on (or beyond) the region's boundary is
//! [`FitError::UnboundedEnvelope`] with the boundary distance and no envelope, never a result.
//! [`MIC_DEFINED`] says for which primitives a maximum inscribed fit is defined at all.
//!
//! # Pinned rules (REQ-KRN.ENV.MIC.TR-001 .. -004)
//!
//! 1. **Defined primitives** (REQ-004). [`MIC_DEFINED`] is `circle2`, `sphere`, `cylinder`.
//!    [`check_defined`] returns `FitError::MethodUndefined{primitive, method:
//!    MaximumInscribed}` for every other primitive (slot and rectangle included: no inscribed
//!    envelope definition is cited for them yet, see the parent node `KRN.ENV.MIC`; the 3D
//!    circle too: a caller projects to `circle2`).
//! 2. **Construction** (REQ-001). [`TrustRegion::from_ls`] takes a least-squares
//!    [`FitResult`] (its algorithm's method segment must be `ls`, else
//!    `InvalidParams{field: "result"}`) and [`TrustRegion::from_feature`] the fitted feature
//!    itself. The feature's primitive must pass rule 1, then it is made canonical
//!    ([`FittedFeature::canonical`]: finite, radius positive, cylinder axis unit length and
//!    sign-normalised). `fraction` must be finite and `> 0`
//!    (`InvalidParams{field: "fraction"}`; there is no default, see the leaf's Open
//!    questions). Then, each one IEEE-754 binary64 operation rounded to nearest (no FMA):
//!    `radius_m = fraction * r` and `inradius_m = radius_m * K`, with `K = K2 = cos(pi/32)`
//!    for `circle2` and `cylinder` and `K = K3` for `sphere` (rule 3). A `radius_m` or
//!    `inradius_m` that is not a finite normal number (overflow, or underflow below
//!    `2^-1022`) is `InvalidParams{field: "fraction"}`. The centre is the circle's centre with
//!    `z = +0.0`, the sphere's centre, or the cylinder's axis point; `axis` is the canonical
//!    cylinder axis (`None` otherwise). `fraction`, `radius_m` and `inradius_m` are kept in
//!    the record so the fit can record them in its parameters.
//! 3. **Rows** (REQ-002). The region is the polygon or polyhedron `{x : a_k . x <= b_k}`
//!    inscribed in the ball of radius `radius_m` about the centre (its farthest points, the
//!    vertices, are at `radius_m`, up to rounding), with facet normals from committed constant
//!    tables (no runtime transcendentals) and `b_k = red::dot(a_k, centre) + inradius_m`:
//!    - `circle2`: the regular 32-gon, `a_k = (cos(k pi/16), sin(k pi/16), 0)`, `k = 0..31`
//!      ([`DIRS2`]); its apothem is `radius_m * cos(pi/32)`.
//!    - `sphere`: the 26-facet polyhedron with the normals [`DIRS3`] (the 6 axes, the 12
//!      `(+-1, +-1, 0)/sqrt(2)` and the 8 `(+-1, +-1, +-1)/sqrt(3)` directions). With every
//!      plane at distance 1 its farthest vertex, `(1, sqrt(2) - 1, sqrt(3) - sqrt(2))` up to
//!      symmetry, is at `sqrt(9 - 2 sqrt(2) - 2 sqrt(6))`, so `K3 = 1 / sqrt(9 - 2 sqrt(2) -
//!      2 sqrt(6))` (about 0.886): the polyhedron's inradius is `0.886 * radius_m`.
//!    - `cylinder`: the 32-gon of `circle2` in the plane normal to the axis `d`, which makes
//!      the region a prism along the axis (the axis point may slide along the axis; only its
//!      distance from the least-squares axis is held). The plane basis: `j` is the first index
//!      of the smallest `|d_j|` (total order); `t = e_j - d_j d` (each component one product
//!      and one difference); `e1 = t / red::norm2(t)` with `-0.0` made `+0.0`;
//!      `e2 = d x e1` (each component two products and one difference, not renormalised);
//!      `a_k = cos(k pi/16) e1 + sin(k pi/16) e2` (each component two products and one sum,
//!      not renormalised).
//!
//!    The tables hold correctly rounded binary64 values, so the normals are unit length to
//!    within rounding and the rows are the same bits on every target.
//! 4. **Boundary check** (REQ-003). `boundary_tol` must be finite and `>= 0`
//!    (`InvalidParams{field: "boundary_tol"}`; `-0.0` is accepted as `+0.0`); a non-finite
//!    position is `Num`. The slack of facet `k` is `b_k - red::dot(a_k, x)`, and the boundary
//!    distance is the total-order minimum of the slacks (the distance to the nearest facet
//!    plane, negative outside the region). A distance `<= boundary_tol` is
//!    `UnboundedEnvelope{boundary_distance_m}`; otherwise the check passes and returns the
//!    distance. For `circle2` the position's `z` is multiplied by the rows' `0.0` and so does
//!    not count, but it must be finite.
//!
//! Everything is `fid_num::red` sums, plain binary64 products, sums and divisions and
//! total-order comparisons: no FMA, no `f64::min`/`f64::max`, no hashing, so every value is
//! bit-identical on x86-64, ARM64 and wasm32 (D0).

use fid_num::{check_finite_slice, f, red};

use crate::contract::{unit_direction, FitError, FitResult, FittedFeature, Method, Primitive, V3};

const OP_ROWS: &str = "mic_tr.rows";
const OP_ALLOC: &str = "mic_tr";

/// The primitives with a defined maximum inscribed fit (REQ-KRN.ENV.MIC.TR-004), in
/// [`Primitive`] declaration order.
pub const MIC_DEFINED: [Primitive; 3] = [Primitive::Circle2, Primitive::Sphere, Primitive::Cylinder];

/// Facet count of the 2D polygon (and of the cylinder's prism).
pub const FACETS_2D: usize = 32;

/// Facet count of the 3D polyhedron.
pub const FACETS_3D: usize = 26;

/// `cos(pi / 32)`, correctly rounded: the apothem of the regular 32-gon of circumradius 1.
pub const K2: f64 = 0.9951847266721969;

/// `1 / sqrt(9 - 2 sqrt(2) - 2 sqrt(6))`, correctly rounded: the inradius of the [`DIRS3`]
/// polyhedron whose farthest vertex is at distance 1.
pub const K3: f64 = 0.8864518862828913;

// cos(j pi / 16) for j = 0..8, correctly rounded (C8 = cos(pi/2) = 0).
const C1: f64 = 0.9807852804032304;
const C2: f64 = 0.9238795325112867;
const C3: f64 = 0.8314696123025452;
const C4: f64 = 0.7071067811865476;
const C5: f64 = 0.5555702330196022;
const C6: f64 = 0.3826834323650898;
const C7: f64 = 0.19509032201612828;
/// `1 / sqrt(2)`, correctly rounded (the same value as `C4`).
const R2: f64 = 0.7071067811865476;
/// `1 / sqrt(3)`, correctly rounded.
const R3: f64 = 0.5773502691896257;

/// The 2D facet normals `(cos(k pi/16), sin(k pi/16))`, `k = 0..31`.
pub const DIRS2: [[f64; 2]; FACETS_2D] = [
    [1.0, 0.0],
    [C1, C7],
    [C2, C6],
    [C3, C5],
    [C4, C4],
    [C5, C3],
    [C6, C2],
    [C7, C1],
    [0.0, 1.0],
    [-C7, C1],
    [-C6, C2],
    [-C5, C3],
    [-C4, C4],
    [-C3, C5],
    [-C2, C6],
    [-C1, C7],
    [-1.0, 0.0],
    [-C1, -C7],
    [-C2, -C6],
    [-C3, -C5],
    [-C4, -C4],
    [-C5, -C3],
    [-C6, -C2],
    [-C7, -C1],
    [0.0, -1.0],
    [C7, -C1],
    [C6, -C2],
    [C5, -C3],
    [C4, -C4],
    [C3, -C5],
    [C2, -C6],
    [C1, -C7],
];

/// The 3D facet normals: `+x, -x, +y, -y, +z, -z`; then `(+-1, +-1, 0)`, `(+-1, 0, +-1)`,
/// `(0, +-1, +-1)` over `sqrt(2)` (signs `++, +-, -+, --`); then `(+-1, +-1, +-1)` over
/// `sqrt(3)` (signs in the order `+++, ++-, +-+, +--, -++, -+-, --+, ---`).
pub const DIRS3: [V3; FACETS_3D] = [
    [1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [0.0, -1.0, 0.0],
    [0.0, 0.0, 1.0],
    [0.0, 0.0, -1.0],
    [R2, R2, 0.0],
    [R2, -R2, 0.0],
    [-R2, R2, 0.0],
    [-R2, -R2, 0.0],
    [R2, 0.0, R2],
    [R2, 0.0, -R2],
    [-R2, 0.0, R2],
    [-R2, 0.0, -R2],
    [0.0, R2, R2],
    [0.0, R2, -R2],
    [0.0, -R2, R2],
    [0.0, -R2, -R2],
    [R3, R3, R3],
    [R3, R3, -R3],
    [R3, -R3, R3],
    [R3, -R3, -R3],
    [-R3, R3, R3],
    [-R3, R3, -R3],
    [-R3, -R3, R3],
    [-R3, -R3, -R3],
];

/// Whether a maximum inscribed fit is defined for `p` (rule 1).
pub fn is_defined(p: Primitive) -> bool {
    MIC_DEFINED.contains(&p)
}

/// `Ok` for a primitive in [`MIC_DEFINED`]; otherwise
/// `FitError::MethodUndefined{primitive, method: MaximumInscribed}` (code `method_undefined`).
pub fn check_defined(p: Primitive) -> Result<(), FitError> {
    if is_defined(p) {
        Ok(())
    } else {
        Err(FitError::MethodUndefined { primitive: p, method: Method::MaximumInscribed })
    }
}

/// The trust region of one maximum inscribed fit (REQ-KRN.ENV.MIC.TR-001). Built only by
/// [`TrustRegion::from_ls`] or [`TrustRegion::from_feature`], so every value is validated.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrustRegion {
    primitive: Primitive,
    centre: V3,
    axis: Option<V3>,
    radius_m: f64,
    inradius_m: f64,
    fraction: f64,
}

/// The region as linear constraints `a[k] . x <= b[k]` (REQ-KRN.ENV.MIC.TR-002): 32 rows for
/// `circle2` and `cylinder`, 26 for `sphere`, in table order.
#[derive(Debug, Clone, PartialEq)]
pub struct TrustRows {
    pub a: Vec<V3>,
    pub b: Vec<f64>,
}

fn rows_vec<T>(n: usize) -> Result<Vec<T>, FitError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)
        .map_err(|_| FitError::Num(fid_num::NumError::Domain { op: OP_ALLOC, detail: "allocation_failed" }))?;
    Ok(v)
}

fn finite_normal(x: f64) -> bool {
    x.is_finite() && x >= f64::MIN_POSITIVE
}

impl TrustRegion {
    /// The region about a least-squares result (rule 2). `InvalidParams{field: "result"}`
    /// when the result's algorithm is not a least-squares one (method segment `ls`).
    pub fn from_ls(result: &FitResult, fraction: f64) -> Result<TrustRegion, FitError> {
        if result.algorithm.method != Method::LeastSquares.slug() {
            return Err(FitError::InvalidParams { field: "result" });
        }
        TrustRegion::from_feature(&result.feature, fraction)
    }

    /// The region about a least-squares feature (rule 2).
    pub fn from_feature(feature: &FittedFeature, fraction: f64) -> Result<TrustRegion, FitError> {
        let primitive = feature.primitive();
        check_defined(primitive)?;
        if !(fraction.is_finite() && fraction > 0.0) {
            return Err(FitError::InvalidParams { field: "fraction" });
        }
        let (centre, axis, r, k) = match feature.canonical()? {
            FittedFeature::Circle2 { c, r } => ([c[0], c[1], 0.0], None, r, K2),
            FittedFeature::Sphere { c, r } => (c, None, r, K3),
            FittedFeature::Cylinder { p, d, r } => (p, Some(d), r, K2),
            // Unreachable after `check_defined`; refused rather than assumed.
            _ => return Err(FitError::MethodUndefined { primitive, method: Method::MaximumInscribed }),
        };
        let radius_m = fraction * r;
        let inradius_m = radius_m * k;
        if !finite_normal(radius_m) || !finite_normal(inradius_m) {
            return Err(FitError::InvalidParams { field: "fraction" });
        }
        Ok(TrustRegion { primitive, centre, axis, radius_m, inradius_m, fraction })
    }

    /// The primitive of the least-squares feature.
    pub fn primitive(&self) -> Primitive {
        self.primitive
    }

    /// The centre (circle: `z = +0.0`) or the cylinder's axis point.
    pub fn centre(&self) -> V3 {
        self.centre
    }

    /// The canonical cylinder axis; `None` for a circle or sphere.
    pub fn axis(&self) -> Option<V3> {
        self.axis
    }

    /// `fraction * r`: the circumradius of the region (metres).
    pub fn radius_m(&self) -> f64 {
        self.radius_m
    }

    /// `radius_m * K`: the distance from the centre to every facet plane (metres).
    pub fn inradius_m(&self) -> f64 {
        self.inradius_m
    }

    /// The required fraction of the least-squares radius, as given.
    pub fn fraction(&self) -> f64 {
        self.fraction
    }

    /// The facet normals (rule 3).
    fn normals(&self) -> Result<Vec<V3>, FitError> {
        match self.axis {
            None if self.primitive == Primitive::Sphere => {
                let mut a = rows_vec(FACETS_3D)?;
                a.extend_from_slice(&DIRS3);
                Ok(a)
            }
            None => {
                let mut a = rows_vec(FACETS_2D)?;
                for u in DIRS2 {
                    a.push([u[0], u[1], 0.0]);
                }
                Ok(a)
            }
            Some(d) => {
                let (e1, e2) = plane_basis(d)?;
                let mut a = rows_vec(FACETS_2D)?;
                for u in DIRS2 {
                    a.push([u[0] * e1[0] + u[1] * e2[0], u[0] * e1[1] + u[1] * e2[1], u[0] * e1[2] + u[1] * e2[2]]);
                }
                Ok(a)
            }
        }
    }

    /// The region as linear constraints (rule 3). `Num` if a `b_k` overflows.
    pub fn rows(&self) -> Result<TrustRows, FitError> {
        let a = self.normals()?;
        let mut b = rows_vec(a.len())?;
        for ak in &a {
            let bk = red::dot(ak, &self.centre)? + self.inradius_m;
            if !bk.is_finite() {
                return Err(FitError::Num(fid_num::NumError::Infinite { op: OP_ROWS, index: Some(b.len()) }));
            }
            b.push(bk);
        }
        Ok(TrustRows { a, b })
    }

    /// The signed distance of `x` from the region's boundary (rule 4): the total-order minimum
    /// over the facets of `b_k - a_k . x`, negative outside. `Num` for a non-finite `x` or an
    /// overflow.
    pub fn boundary_distance(&self, x: V3) -> Result<f64, FitError> {
        check_finite_slice("mic_tr.position", &x)?;
        let rows = self.rows()?;
        let mut min = f64::INFINITY;
        for (ak, bk) in rows.a.iter().zip(rows.b.iter()) {
            let s = *bk - red::dot(ak, &x)?;
            if !s.is_finite() {
                return Err(FitError::Num(fid_num::NumError::Infinite { op: "mic_tr.slack", index: None }));
            }
            min = f::total_min(min, s)?;
        }
        Ok(min)
    }

    /// The boundary check of a final centre or axis point (REQ-KRN.ENV.MIC.TR-003, rule 4):
    /// `UnboundedEnvelope{boundary_distance_m}` when [`TrustRegion::boundary_distance`] is
    /// `<= boundary_tol`, otherwise that distance.
    pub fn check_boundary(&self, x: V3, boundary_tol: f64) -> Result<f64, FitError> {
        if !(boundary_tol.is_finite() && boundary_tol >= 0.0) {
            return Err(FitError::InvalidParams { field: "boundary_tol" });
        }
        let dist = self.boundary_distance(x)?;
        if dist <= boundary_tol {
            return Err(FitError::UnboundedEnvelope { boundary_distance_m: dist });
        }
        Ok(dist)
    }
}

/// The basis `(e1, e2)` of the plane normal to the unit axis `d` (rule 3, cylinder).
fn plane_basis(d: V3) -> Result<(V3, V3), FitError> {
    let mut j = 0;
    for (i, di) in d.iter().enumerate().skip(1) {
        if f::total_cmp(di.abs(), d[j].abs())? == core::cmp::Ordering::Less {
            j = i;
        }
    }
    let mut t = [0.0; 3];
    for (i, ti) in t.iter_mut().enumerate() {
        let e = if i == j { 1.0 } else { 0.0 };
        *ti = e - d[j] * d[i];
    }
    let e1 = unit_direction(t)?;
    let e2 = [d[1] * e1[2] - d[2] * e1[1], d[2] * e1[0] - d[0] * e1[2], d[0] * e1[1] - d[1] * e1[0]];
    Ok((e1, e2))
}
