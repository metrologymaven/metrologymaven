//! The station-plane profile of the sheet end: its segments, its samples and the analytic
//! truths (tangency break and projected silhouette) of `SCAN.EDGE.EST.EFIX`.
//!
//! Coordinates are `(u, v)` in metres: `u` along the station's outward direction `b`, `v`
//! along the nominal top-face normal `n`. The sheet occupies `v` in `[-T, 0]` and the actual
//! edge face (the trim line) is at `u = e`, the trim offset. The segment layout per profile is
//! pinned in the [module documentation](super).

use core::f64::consts::FRAC_PI_2;

use super::{EdgeFixtureSpec, EdgeProfile, EfixError};

/// A profile sample: the point `(u, v)` and its unit normal.
pub(crate) type Sample = ([f64; 2], [f64; 2]);

/// Which way a segment faces, from its normal's `v` component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Facing {
    /// Toward `+n` (the top face, the chamfer, the burr ledge) or the round's quarter arc,
    /// whose normal turns from `+n` to `+b`.
    Up,
    /// Normal along `+b` (an edge face, the burr face).
    Vertical,
    /// Toward `-n` (the bottom face).
    Down,
}

/// The shape of a segment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    /// A straight segment with its unit normal.
    Line { normal: [f64; 2] },
    /// The round's quarter arc: centre and radius; the point at angle `phi` (0 at the top,
    /// `pi/2` at the edge face) is `centre + r (sin phi, cos phi)` with normal
    /// `(sin phi, cos phi)`.
    Arc { centre: [f64; 2], radius: f64 },
}

/// One profile segment, from `start` to `end` in profile order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Segment {
    pub(crate) shape: Shape,
    pub(crate) start: [f64; 2],
    pub(crate) end: [f64; 2],
    pub(crate) length: f64,
    pub(crate) facing: Facing,
}

impl Segment {
    fn line(start: [f64; 2], end: [f64; 2]) -> Result<Segment, EfixError> {
        let du = end[0] - start[0];
        let dv = end[1] - start[1];
        let length = fid_num::f::sqrt(du * du + dv * dv)?;
        if length <= 0.0 {
            return Err(EfixError::OutOfRange { field: "profile", reason: "a profile segment has zero length" });
        }
        let normal = [(0.0 - dv) / length, du / length];
        let facing = if normal[1] > 0.0 {
            Facing::Up
        } else if normal[1] < 0.0 {
            Facing::Down
        } else {
            Facing::Vertical
        };
        Ok(Segment { shape: Shape::Line { normal }, start, end, length, facing })
    }

    fn quarter_arc(start: [f64; 2], end: [f64; 2], centre: [f64; 2], radius: f64) -> Segment {
        Segment {
            shape: Shape::Arc { centre, radius },
            start,
            end,
            length: radius * (FRAC_PI_2 - 0.0),
            facing: Facing::Up,
        }
    }

    /// The analytic unit normal at the segment's start.
    pub(crate) fn start_normal(&self) -> [f64; 2] {
        match self.shape {
            Shape::Line { normal } => normal,
            Shape::Arc { .. } => [0.0, 1.0],
        }
    }

    /// The analytic unit normal at the segment's end.
    pub(crate) fn end_normal(&self) -> [f64; 2] {
        match self.shape {
            Shape::Line { normal } => normal,
            Shape::Arc { .. } => [1.0, 0.0],
        }
    }

    /// The point and unit normal at fraction `f` in `(0, 1)` of the segment.
    fn interior(&self, f: f64) -> Result<Sample, EfixError> {
        match self.shape {
            Shape::Line { normal } => {
                let p = [
                    self.start[0] + (self.end[0] - self.start[0]) * f,
                    self.start[1] + (self.end[1] - self.start[1]) * f,
                ];
                Ok((p, normal))
            }
            Shape::Arc { centre, radius } => {
                let phi = FRAC_PI_2 * f;
                let (s, c) = (fid_num::f::sin(phi)?, fid_num::f::cos(phi)?);
                Ok(([centre[0] + radius * s, centre[1] + radius * c], [s, c]))
            }
        }
    }
}

/// The full profile (top face, edge, bottom face) of `spec`, in profile order. The spec has
/// been validated.
pub(crate) fn segments(spec: &EdgeFixtureSpec) -> Result<Vec<Segment>, EfixError> {
    let t = spec.thickness_m;
    let e = spec.trim_offset_m;
    let top = [e - spec.top_extent_m, 0.0];
    let mut segs = Vec::new();
    match spec.profile {
        EdgeProfile::Sharp => {
            segs.push(Segment::line(top, [e, 0.0])?);
            segs.push(Segment::line([e, 0.0], [e, 0.0 - t])?);
            segs.push(Segment::line([e, 0.0 - t], [e - spec.bottom_extent_m, 0.0 - t])?);
        }
        EdgeProfile::Round { radius_m: r } => {
            segs.push(Segment::line(top, [e - r, 0.0])?);
            segs.push(Segment::quarter_arc([e - r, 0.0], [e, 0.0 - r], [e - r, 0.0 - r], r));
            if r < t {
                segs.push(Segment::line([e, 0.0 - r], [e, 0.0 - t])?);
            }
            segs.push(Segment::line([e, 0.0 - t], [e - spec.bottom_extent_m, 0.0 - t])?);
        }
        EdgeProfile::Chamfer { width_m: c } => {
            segs.push(Segment::line(top, [e - c, 0.0])?);
            segs.push(Segment::line([e - c, 0.0], [e, 0.0 - c])?);
            if c < t {
                segs.push(Segment::line([e, 0.0 - c], [e, 0.0 - t])?);
            }
            segs.push(Segment::line([e, 0.0 - t], [e - spec.bottom_extent_m, 0.0 - t])?);
        }
        EdgeProfile::Burr { height_m: h, width_m: w } => {
            let ledge = 0.0 - (t - h);
            segs.push(Segment::line(top, [e, 0.0])?);
            segs.push(Segment::line([e, 0.0], [e, ledge])?);
            segs.push(Segment::line([e, ledge], [e + w, ledge])?);
            segs.push(Segment::line([e + w, ledge], [e + w, 0.0 - t])?);
            segs.push(Segment::line([e + w, 0.0 - t], [e - spec.bottom_extent_m, 0.0 - t])?);
        }
    }
    Ok(segs)
}

/// Which segments are covered: all of them for wrapped coverage. For one-sided coverage, the
/// upward-facing segments visible from `+n`: those whose `u` increases from a start at or beyond
/// the largest `u` of every earlier profile point (the top face, the round's arc or the chamfer,
/// and the burr's ledge, which stands out past the edge face).
pub(crate) fn covered(segs: &[Segment], one_sided: bool) -> Vec<bool> {
    if !one_sided {
        return vec![true; segs.len()];
    }
    let mut out = Vec::with_capacity(segs.len());
    let mut reach = f64::NEG_INFINITY;
    for seg in segs {
        let visible = seg.facing == Facing::Up && seg.start[0] >= reach && seg.end[0] > seg.start[0];
        out.push(visible);
        for u in [seg.start[0], seg.end[0]] {
            if u > reach {
                reach = u;
            }
        }
    }
    out
}

/// The interval count of a segment (or of the path): `max(1, ceil(length / spacing))`, as a
/// float so the caller can bound it before converting.
pub(crate) fn intervals(length: f64, spacing: f64) -> f64 {
    let m = (length / spacing).ceil();
    if m < 1.0 {
        1.0
    } else {
        m
    }
}

/// The unit normal at a junction: the common normal when both sides agree, otherwise the
/// normalised bisector of the incoming and outgoing normals.
fn junction_normal(a: [f64; 2], b: [f64; 2]) -> Result<[f64; 2], EfixError> {
    if a == b {
        return Ok(a);
    }
    let s = [a[0] + b[0], a[1] + b[1]];
    let len = fid_num::f::sqrt(s[0] * s[0] + s[1] * s[1])?;
    if len <= 0.0 {
        return Err(EfixError::OutOfRange { field: "profile", reason: "a profile fold reverses the normal" });
    }
    Ok([s[0] / len, s[1] / len])
}

/// The profile samples of the covered segments with their unit normals, in profile order, and
/// for each sample whether it joins the previous one (a mesh cell spans the two columns).
pub(crate) struct ProfileSamples {
    pub(crate) points: Vec<Sample>,
    pub(crate) joined: Vec<bool>,
}

/// Samples the covered segments: every segment's end points exactly and `m - 1` evenly spaced
/// interior points (`m` from [`intervals`]); a junction point shared by two covered segments
/// once. A covered segment after an uncovered one starts a new piece (`joined` false), so no mesh
/// cell bridges unscanned surface. A junction point's normal is [`junction_normal`] of the two
/// segments meeting there, also where the neighbour is not covered (a property of the part, not
/// of the coverage).
pub(crate) fn samples(segs: &[Segment], covered: &[bool], spacing: f64) -> Result<ProfileSamples, EfixError> {
    let mut points = Vec::new();
    let mut joined = Vec::new();
    let mut prev_covered = false;
    for (i, seg) in segs.iter().enumerate() {
        let is_covered = covered.get(i).copied().unwrap_or(false);
        if !is_covered {
            prev_covered = false;
            continue;
        }
        if !prev_covered {
            let n_start = match i.checked_sub(1).and_then(|k| segs.get(k)) {
                Some(prev) => junction_normal(prev.end_normal(), seg.start_normal())?,
                None => seg.start_normal(),
            };
            points.push((seg.start, n_start));
            joined.push(false);
        }
        let m = intervals(seg.length, spacing);
        let mut j = 1.0;
        while j < m {
            points.push(seg.interior(j / m)?);
            joined.push(true);
            j += 1.0;
        }
        let n_end = match segs.get(i + 1) {
            Some(next) => junction_normal(seg.end_normal(), next.start_normal())?,
            None => seg.end_normal(),
        };
        points.push((seg.end, n_end));
        joined.push(true);
        prev_covered = true;
    }
    Ok(ProfileSamples { points, joined })
}

/// The tangency-break truth for threshold `theta` (radians, in `(0, pi)`): the first profile
/// point at which the normal's angle to `n` exceeds `theta`. The angle exceeds `theta` exactly
/// when the normal's `v` component is below `cos(theta)`. On the round's arc that point is the
/// one at angle `theta`; at a corner it is the start of the segment after it. `None` if no
/// point of the profile exceeds `theta`.
pub(crate) fn tangency(segs: &[Segment], theta: f64) -> Result<Option<[f64; 2]>, EfixError> {
    let c = fid_num::f::cos(theta)?;
    for seg in segs {
        if seg.start_normal()[1] < c {
            return Ok(Some(seg.start));
        }
        if let Shape::Arc { centre, radius } = seg.shape {
            if seg.end_normal()[1] < c {
                let (s, co) = (fid_num::f::sin(theta)?, fid_num::f::cos(theta)?);
                return Ok(Some([centre[0] + radius * s, centre[1] + radius * co]));
            }
        }
    }
    Ok(None)
}

/// The projected-silhouette truth: the profile point with the largest `u`, the first in
/// profile order if several. On every pinned profile the maximum is at a segment end point
/// (the round's arc has its largest `u` at its end).
pub(crate) fn silhouette(segs: &[Segment]) -> [f64; 2] {
    let mut best = match segs.first() {
        Some(s) => s.start,
        None => [0.0, 0.0],
    };
    for seg in segs {
        if seg.end[0] > best[0] {
            best = seg.end;
        }
    }
    best
}
