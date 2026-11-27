//! Section plane definitions and series. Spec node `SCAN.SECT.DEF`.
//!
//! A [`SectionDef`] says which cutting planes a section uses; [`expand`] turns it into an
//! ordered list of [`Plane`]s (point and unit normal, metres, in the section's frame),
//! [`validate`] refuses a bad definition with `scan.sect.bad_definition` and a [`Reason`], and
//! [`digest`] is the SHA-256 of its [canonical JSON](canonical_json).
//!
//! # Variants and rules
//!
//! - **Single**: one [`Plane`].
//! - **Parallel** (REQ-001): base plane `(p0, n)`, spacing `s > 0`, and either a count `k` or
//!   an extent `e >= 0`. Plane `i` (for `i = 0 .. k-1`) has point `p0 + t n` with
//!   `t = (i as f64) * s`, each component `p0[c] + t * n[c]` (plain products, no FMA), and
//!   normal `n` as given (unit within `1e-12`, not re-normalised). With an extent, the planes
//!   are those whose offset `t` is at most `e + e * 1e-12` (the **closed** range `[0, e]`, with
//!   a relative slack far above the rounding of `i * s`, so an extent of `0.04` with spacing
//!   `0.01` gives 5 planes).
//! - **Radial** (REQ-002): axis point `a`, unit axis direction `d`, unit reference direction
//!   `r` with `|d . r| <= 1e-12`, step `theta > 0`, and either a count `k` or a sweep `w > 0`.
//!   Plane `i` contains the axis (its point is `a`); with `phi = (i as f64) * theta` and
//!   `c = cos phi`, `s = sin phi` from `fid-num`, the reference direction is rotated by
//!   Rodrigues' formula `rot = (r c + (d x r) s) + (d (d . r)) (1 - c)` (each component in that
//!   order), and the normal is `d x rot` divided by its `fid_num::red::norm2`. So plane `i` is
//!   the half-plane from the axis towards `rot`, and planes `phi` and `phi + pi` are distinct
//!   (opposite normals). With a sweep, the planes are `i = 0` and every `i >= 1` with
//!   `i theta < w - 1e-12` (the **half-open** range `[0, w)`: a radial series is periodic, so a
//!   sweep of `2 pi` with step `pi / 2` gives 4 planes, not a fifth repeating the first). A
//!   series whose last angle reaches a full turn, `(k - 1) theta >= 2 pi - 1e-12`, would repeat
//!   its first plane and is refused ([`Reason::RadialFullTurn`]).
//! - **FromElement** (REQ-003): the plane of a planar CAD face or a constructed plane, named by
//!   an [`ElementRef`] and resolved through an [`ElementResolver`] (see [`ModelResolver`] for
//!   which plane and which digest). [`FromElement::record`] resolves the element once and
//!   records its ID and digest; [`validate`] and [`expand`] resolve it again and refuse with
//!   [`Reason::ElementStale`] when the digest changed, so a section never silently follows an
//!   edited element: it is recomputed from a re-recorded definition.
//!
//! # Validation (REQ-004)
//!
//! Every coordinate and scalar must be finite ([`Reason::NonFinite`]); normals and directions
//! unit length within `1e-12` (`|norm2(v) - 1| <= 1e-12`, [`Reason::NotUnit`]); spacing and
//! step positive; a count between 1 and [`MAX_PLANES`] (given or derived from an extent or a
//! sweep); and every expanded plane finite ([`Reason::PlaneNotFinite`]). The constructors
//! ([`Plane::new`], [`Parallel::new`], [`Radial::new`], [`FromElement::record`]) apply these
//! checks and expand the series once, so a definition that exists is valid on its own;
//! [`validate`] expands it again and resolves a `FromElement`, which can go stale. All tolerances are fixed absolute or relative values (`1e-12`),
//! far above binary64 rounding of the quantities they bound.
//!
//! # Determinism (REQ-005)
//!
//! Pure, D0: plain binary64 arithmetic in a fixed order, `sin`, `cos`, `norm2` and `dot`
//! through `fid-num` (bit-identical on x86-64, ARM64 and wasm32), no FMA, no `HashMap`, no
//! `f64::min` or `f64::max`. The constructors store `-0.0` as `+0.0` (inputs and expanded
//! planes), so a definition has one canonical encoding.

mod error;
mod json;
mod resolve;

pub use error::{Reason, SectError};
pub use json::{canonical_json, FORMAT};
pub use resolve::{
    ElementRef, ElementResolver, ModelResolver, NoElements, ResolveError, Resolved, ResolvedGeometry,
    ResolverBuildError,
};

use core::f64::consts::TAU;

use fid_kci::kci::sha256_digest;
use fid_num::{f, red};
use fid_types::Digest;

/// Most planes one definition may expand to.
pub const MAX_PLANES: u32 = 10_000;

/// Unit-length and perpendicularity tolerance of normals and directions.
pub const UNIT_TOL: f64 = 1e-12;

/// Relative slack of a parallel extent: offsets up to `e + e * EXTENT_SLACK` are inside.
pub const EXTENT_SLACK: f64 = 1e-12;

/// Absolute slack (radians) of a radial sweep end and of the full-turn check.
pub const ANGLE_SLACK: f64 = 1e-12;

/// `-0.0` as `+0.0`; every other value unchanged.
fn canon(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        v
    }
}

fn canon3(v: [f64; 3]) -> [f64; 3] {
    [canon(v[0]), canon(v[1]), canon(v[2])]
}

fn finite(field: &'static str, v: f64) -> Result<f64, SectError> {
    if v.is_finite() {
        Ok(canon(v))
    } else {
        Err(Reason::NonFinite { field }.into())
    }
}

fn finite3(field: &'static str, v: [f64; 3]) -> Result<[f64; 3], SectError> {
    Ok([finite(field, v[0])?, finite(field, v[1])?, finite(field, v[2])?])
}

fn unit3(field: &'static str, v: [f64; 3]) -> Result<[f64; 3], SectError> {
    let v = finite3(field, v)?;
    let len = red::norm2(&v)?;
    if (len - 1.0).abs() > UNIT_TOL {
        return Err(Reason::NotUnit { field }.into());
    }
    Ok(v)
}

fn count(k: u32) -> Result<u32, SectError> {
    if k == 0 {
        Err(Reason::CountZero.into())
    } else if k > MAX_PLANES {
        Err(Reason::CountTooLarge.into())
    } else {
        Ok(k)
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// A plane: a point and a unit normal (metres), finite, `-0.0` stored as `+0.0`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Plane {
    point: [f64; 3],
    normal: [f64; 3],
}

impl Plane {
    /// A checked plane: finite point ([`Reason::NonFinite`] `plane.point`), normal unit length
    /// within `1e-12` ([`Reason::NotUnit`] `plane.normal`). The normal is kept as given (not
    /// re-normalised).
    pub fn new(point: [f64; 3], normal: [f64; 3]) -> Result<Plane, SectError> {
        Plane::checked("plane.point", point, "plane.normal", normal)
    }

    fn checked(pf: &'static str, point: [f64; 3], nf: &'static str, normal: [f64; 3]) -> Result<Plane, SectError> {
        Ok(Plane { point: finite3(pf, point)?, normal: unit3(nf, normal)? })
    }

    pub fn point(&self) -> [f64; 3] {
        self.point
    }

    pub fn normal(&self) -> [f64; 3] {
        self.normal
    }
}

/// How far a parallel series runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ParallelSpan {
    /// Exactly this many planes.
    Count(u32),
    /// The planes with offset in the closed range `[0, extent]` (metres; module documentation).
    Extent(f64),
}

/// How far a radial series runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RadialSpan {
    /// Exactly this many planes.
    Count(u32),
    /// The planes with angle in the half-open range `[0, sweep)` (radians; module documentation).
    Sweep(f64),
}

/// A parallel plane series (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Parallel {
    base: Plane,
    spacing: f64,
    span: ParallelSpan,
    count: u32,
}

impl Parallel {
    /// A checked series: spacing finite and positive, count in `1 ..= MAX_PLANES` (given, or
    /// derived from a finite, non-negative extent), every plane finite.
    pub fn new(base: Plane, spacing: f64, span: ParallelSpan) -> Result<Parallel, SectError> {
        let spacing = finite("parallel.spacing", spacing)?;
        if spacing <= 0.0 {
            return Err(Reason::SpacingNotPositive.into());
        }
        let (span, k) = match span {
            ParallelSpan::Count(k) => (ParallelSpan::Count(k), count(k)?),
            ParallelSpan::Extent(e) => {
                let e = finite("parallel.extent", e)?;
                if e < 0.0 {
                    return Err(Reason::ExtentNegative.into());
                }
                (ParallelSpan::Extent(e), extent_count(spacing, e)?)
            }
        };
        let p = Parallel { base, spacing, span, count: k };
        p.planes()?;
        Ok(p)
    }

    pub fn base(&self) -> &Plane {
        &self.base
    }

    /// Spacing in metres.
    pub fn spacing(&self) -> f64 {
        self.spacing
    }

    /// The span as given.
    pub fn span(&self) -> ParallelSpan {
        self.span
    }

    /// The number of planes (given, or derived from the extent).
    pub fn count(&self) -> u32 {
        self.count
    }

    fn planes(&self) -> Result<Vec<Plane>, SectError> {
        let (p0, n) = (self.base.point, self.base.normal);
        let mut out = Vec::with_capacity(self.count as usize);
        for i in 0..self.count {
            let t = f64::from(i) * self.spacing;
            let p = [p0[0] + t * n[0], p0[1] + t * n[1], p0[2] + t * n[2]];
            if !(p[0].is_finite() && p[1].is_finite() && p[2].is_finite()) {
                return Err(Reason::PlaneNotFinite { index: i }.into());
            }
            out.push(Plane { point: canon3(p), normal: n });
        }
        Ok(out)
    }
}

/// The number of planes with offset `i * s <= e + e * EXTENT_SLACK`; at most `MAX_PLANES`.
fn extent_count(spacing: f64, extent: f64) -> Result<u32, SectError> {
    let limit = extent + extent * EXTENT_SLACK;
    let mut k: u32 = 0;
    while k <= MAX_PLANES {
        if f64::from(k) * spacing > limit {
            return Ok(k);
        }
        k += 1;
    }
    Err(Reason::CountTooLarge.into())
}

/// A radial plane series about an axis (REQ-002).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Radial {
    axis_point: [f64; 3],
    axis_dir: [f64; 3],
    ref_dir: [f64; 3],
    step: f64,
    span: RadialSpan,
    count: u32,
}

impl Radial {
    /// A checked series: finite axis point, unit axis and reference directions (within
    /// `1e-12`), `|d . r| <= 1e-12`, step finite and positive, count in `1 ..= MAX_PLANES`
    /// (given, or derived from a finite positive sweep), and no full turn.
    pub fn new(
        axis_point: [f64; 3],
        axis_dir: [f64; 3],
        ref_dir: [f64; 3],
        step: f64,
        span: RadialSpan,
    ) -> Result<Radial, SectError> {
        let axis_point = finite3("radial.axis_point", axis_point)?;
        let axis_dir = unit3("radial.axis_dir", axis_dir)?;
        let ref_dir = unit3("radial.ref_dir", ref_dir)?;
        if red::dot(&axis_dir, &ref_dir)?.abs() > UNIT_TOL {
            return Err(Reason::NotPerpendicular.into());
        }
        let step = finite("radial.step", step)?;
        if step <= 0.0 {
            return Err(Reason::StepNotPositive.into());
        }
        let (span, k) = match span {
            RadialSpan::Count(k) => (RadialSpan::Count(k), count(k)?),
            RadialSpan::Sweep(w) => {
                let w = finite("radial.sweep", w)?;
                if w <= 0.0 {
                    return Err(Reason::SweepNotPositive.into());
                }
                (RadialSpan::Sweep(w), sweep_count(step, w)?)
            }
        };
        if f64::from(k - 1) * step >= TAU - ANGLE_SLACK {
            return Err(Reason::RadialFullTurn.into());
        }
        let r = Radial { axis_point, axis_dir, ref_dir, step, span, count: k };
        r.planes()?;
        Ok(r)
    }

    pub fn axis_point(&self) -> [f64; 3] {
        self.axis_point
    }

    pub fn axis_dir(&self) -> [f64; 3] {
        self.axis_dir
    }

    pub fn ref_dir(&self) -> [f64; 3] {
        self.ref_dir
    }

    /// Angular step in radians.
    pub fn step(&self) -> f64 {
        self.step
    }

    /// The span as given.
    pub fn span(&self) -> RadialSpan {
        self.span
    }

    /// The number of planes (given, or derived from the sweep).
    pub fn count(&self) -> u32 {
        self.count
    }

    fn planes(&self) -> Result<Vec<Plane>, SectError> {
        let (d, r) = (self.axis_dir, self.ref_dir);
        let dxr = cross(d, r);
        let ddr = red::dot(&d, &r)?;
        let mut out = Vec::with_capacity(self.count as usize);
        for i in 0..self.count {
            let phi = f64::from(i) * self.step;
            let c = f::cos(phi)?;
            let s = f::sin(phi)?;
            let omc = 1.0 - c;
            let rot = [
                r[0] * c + dxr[0] * s + d[0] * ddr * omc,
                r[1] * c + dxr[1] * s + d[1] * ddr * omc,
                r[2] * c + dxr[2] * s + d[2] * ddr * omc,
            ];
            let m = cross(d, rot);
            let len = red::norm2(&m)?;
            if len <= 0.0 {
                return Err(Reason::DegenerateNormal { index: i }.into());
            }
            let n = [m[0] / len, m[1] / len, m[2] / len];
            out.push(Plane { point: self.axis_point, normal: canon3(n) });
        }
        Ok(out)
    }
}

/// The number of planes `i = 0` plus every `i >= 1` with `i * step < sweep - ANGLE_SLACK`; at
/// most `MAX_PLANES`.
fn sweep_count(step: f64, sweep: f64) -> Result<u32, SectError> {
    let limit = sweep - ANGLE_SLACK;
    let mut k: u32 = 1;
    while k <= MAX_PLANES {
        if f64::from(k) * step >= limit {
            return Ok(k);
        }
        k += 1;
    }
    Err(Reason::CountTooLarge.into())
}

/// A plane taken from a planar CAD face or a constructed plane (REQ-003): the element's ID and
/// the digest it had when the definition was recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FromElement {
    element: ElementRef,
    digest: Digest,
}

impl FromElement {
    /// Resolves `element` and records its ID and current digest. Refused when it cannot be
    /// resolved, is not planar, or its plane is not finite with a unit normal.
    pub fn record<R: ElementResolver + ?Sized>(element: ElementRef, resolver: &R) -> Result<FromElement, SectError> {
        let resolved = resolve(&element, resolver)?;
        plane_of(&resolved)?;
        Ok(FromElement { element, digest: resolved.digest })
    }

    /// A definition read back from storage: the digest is what the stored definition
    /// **claims**; it is checked against the element whenever the definition is validated
    /// or expanded ([`Reason::ElementStale`] on mismatch).
    pub fn recorded(element: ElementRef, digest: Digest) -> FromElement {
        FromElement { element, digest }
    }

    pub fn element(&self) -> &ElementRef {
        &self.element
    }

    /// The element digest recorded with the definition.
    pub fn digest(&self) -> &Digest {
        &self.digest
    }

    fn plane<R: ElementResolver + ?Sized>(&self, resolver: &R) -> Result<Plane, SectError> {
        let resolved = resolve(&self.element, resolver)?;
        if resolved.digest != self.digest {
            return Err(Reason::ElementStale { recorded: self.digest.clone(), current: resolved.digest }.into());
        }
        plane_of(&resolved)
    }
}

fn resolve<R: ElementResolver + ?Sized>(element: &ElementRef, resolver: &R) -> Result<Resolved, SectError> {
    resolver.resolve(element).map_err(|e| {
        SectError::from(match e {
            ResolveError::NotFound => Reason::ElementNotFound,
            ResolveError::NotAFace { kind } => Reason::ElementNotAFace { kind },
            ResolveError::Upstream { code } => Reason::ElementUpstream { code },
        })
    })
}

fn plane_of(resolved: &Resolved) -> Result<Plane, SectError> {
    match resolved.geometry {
        ResolvedGeometry::Plane { point, normal } => Plane::checked("element.point", point, "element.normal", normal),
        ResolvedGeometry::NotPlanar { kind } => Err(Reason::ElementNotPlanar { kind }.into()),
    }
}

/// A section definition.
#[derive(Debug, Clone, PartialEq)]
pub enum SectionDef {
    Single(Plane),
    Parallel(Parallel),
    Radial(Radial),
    FromElement(FromElement),
}

/// Checks `def` (module documentation); `resolver` is used only by `FromElement`.
pub fn validate<R: ElementResolver + ?Sized>(def: &SectionDef, resolver: &R) -> Result<(), SectError> {
    expand(def, resolver).map(|_| ())
}

/// The ordered cutting planes of `def` (module documentation).
pub fn expand<R: ElementResolver + ?Sized>(def: &SectionDef, resolver: &R) -> Result<Vec<Plane>, SectError> {
    match def {
        SectionDef::Single(p) => Ok(vec![Plane::checked("plane.point", p.point, "plane.normal", p.normal)?]),
        SectionDef::Parallel(p) => p.planes(),
        SectionDef::Radial(r) => r.planes(),
        SectionDef::FromElement(f) => Ok(vec![f.plane(resolver)?]),
    }
}

/// SHA-256 of [`canonical_json`], in the `sha256:` wire form (REQ-005, AC-5).
pub fn digest(def: &SectionDef) -> Digest {
    sha256_digest(&canonical_json(def))
}
