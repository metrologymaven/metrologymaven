//! Stride sampling and the overlap fraction (REQ-003, REQ-004).
//!
//! # Stride sampling (pinned, REQ-004)
//!
//! [`stride_sample`]`(n, m)` yields the indexes `floor(i * n / m)` for `i = 0 .. m - 1` when
//! `n > m` (exact integer arithmetic in `u128`), and `0 .. n - 1` otherwise. `m = 0` is refused
//! (`scan.cloud.invalid_parameter`).
//!
//! # Overlap fraction (pinned, REQ-003; checks in this order, first failure wins)
//!
//! [`overlap_fraction`]`(moving, pose, target, d, max_samples)`:
//!
//! 1. `moving` and `target` are in the same frame, else `geo.frame.mismatch`. The pose is a
//!    rigid motion within that frame (for example the current registration estimate); a scan
//!    in another frame is first brought into the target's frame with GEO.FRAME.APPLY, which
//!    records the conversion. Nothing is converted here.
//! 2. `d` is finite and `> 0`; `max_samples >= 1`; `1 <= threads <=` [`MAX_THREADS`]; the
//!    moving cloud is not empty (an empty sample has no fraction). Each refusal is
//!    `scan.cloud.invalid_parameter` naming the parameter.
//! 3. The sample is [`stride_sample`]`(moving.len(), max_samples)`. Each sampled point `x`
//!    (in sample order) is mapped by `Transform::Rigid(pose).apply_point` (the pinned
//!    GEO.FRAME.XFORM expression `(R x)[i] + t[i]`); its image `y` must be finite and within
//!    [`COORD_LIMIT`](super::target::COORD_LIMIT) (else `scan.cloud.invalid_parameter`, `pose`, naming the original index).
//! 4. Rounding floor: with `M` the largest magnitude among the target's valid coordinates, the
//!    sampled `x`, the images `y` and the translation `t`, `d` must be at least
//!    [`FLOOR_FACTOR`]` * 2^-53 * M`; a smaller `d` is below what the computed distances can
//!    resolve and is refused (`scan.cloud.invalid_parameter`, `d`).
//! 5. A sampled point is within when `sqrt(dist2(y, q)) <= d` (`fid_num::f::sqrt`), where `q`
//!    is its nearest valid target point (GEO.INDEX.KDTREE `dist2`). This is the Euclidean
//!    distance to the point, not to its tangent plane.
//!
//! The result is `within` (the count), `sampled` (the sample size) and `fraction = within as
//! f64 / sampled as f64` (each count converted once, then one correctly rounded division).

use core::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use fid_geo_dist::ProjectionTarget;
use fid_geo_frame::{require_same_frame_ref, Rigid, Transform};
use fid_geo_types::PointCloud;

use super::error::RegError;
use super::target::{max_abs, within_limit, OrientedCloudTarget, MAX_THREADS};

/// The rounding floor of `d` in units of `2^-53` times the largest coordinate magnitude
/// (step 4 of the pinned rule). The distance of a sampled point to its nearest target point
/// is computed from a rotated and translated point (a 3-term dot product and an addition per
/// axis), one difference, a 3-term sum of squares and a square root; the accumulated rounding
/// error is, by a first-order count, about `20 * 2^-53 * M`; 64 leaves margin. The floor is a
/// refusal threshold, not a reported error bound.
pub const FLOOR_FACTOR: f64 = 64.0;

const EPS: f64 = f64::from_bits((1023 - 53) << 52);

/// A worker's count, or its lowest failing sample position and the error.
type RangeCount = Result<u64, (u64, RegError)>;

/// Deterministic stride sampling of `n` indexes to at most `m` (see [`stride_sample`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrideSample {
    n: u64,
    m: u64,
    next: u64,
}

impl StrideSample {
    /// The number of indexes the sample holds in total, `min(n, m)`.
    pub fn sample_len(&self) -> u64 {
        if self.n > self.m {
            self.m
        } else {
            self.n
        }
    }

    /// The index at sample position `pos` (`None` beyond the sample).
    pub fn index(&self, pos: u64) -> Option<u64> {
        if pos >= self.sample_len() {
            return None;
        }
        if self.n <= self.m {
            return Some(pos);
        }
        let i = u128::from(pos) * u128::from(self.n) / u128::from(self.m);
        u64::try_from(i).ok()
    }
}

impl Iterator for StrideSample {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        let i = self.index(self.next)?;
        self.next += 1;
        Some(i)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = self.sample_len().saturating_sub(self.next);
        let left = usize::try_from(left).unwrap_or(usize::MAX);
        (left, Some(left))
    }
}

/// The stride sample of `n` indexes to at most `m`: `floor(i * n / m)` for `i < m` when
/// `n > m`, else every index. `m = 0` is `scan.cloud.invalid_parameter`.
pub fn stride_sample(n: u64, m: u64) -> Result<StrideSample, RegError> {
    if m == 0 {
        return Err(RegError::invalid("m", "the sample size must be at least 1".to_string()));
    }
    Ok(StrideSample { n, m, next: 0 })
}

/// The overlap of a moving cloud with a target: how many sampled moving points lie within `d`
/// of a valid target point. The fields are private, so `fraction` always equals
/// `within / sampled` and `within <= sampled`, `sampled >= 1`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overlap {
    within: u64,
    sampled: u64,
    fraction: f64,
}

impl Overlap {
    /// Sampled points within `d` of the target.
    pub fn within(&self) -> u64 {
        self.within
    }

    /// The sample size (at least 1).
    pub fn sampled(&self) -> u64 {
        self.sampled
    }

    /// `within as f64 / sampled as f64`.
    pub fn fraction(&self) -> f64 {
        self.fraction
    }
}

/// The overlap fraction on the calling thread ([`overlap_fraction_with`] with one thread).
pub fn overlap_fraction(
    moving: &PointCloud,
    pose: &Rigid,
    target: &OrientedCloudTarget,
    d: f64,
    max_samples: u64,
) -> Result<Overlap, RegError> {
    overlap_fraction_with(moving, pose, target, d, max_samples, 1)
}

fn image(moving: &PointCloud, pose: &Transform, i: u64) -> Result<([f64; 3], [f64; 3]), RegError> {
    let x = moving.point(i).ok_or(RegError::Inconsistent)?;
    let bad = || RegError::invalid("pose", format!("moving point {i} maps beyond 2^250 m"));
    let y = pose.apply_point(&x).map_err(|_| bad())?;
    if !within_limit(&y) {
        return Err(bad());
    }
    Ok((x, y))
}

fn bigger(m: &mut f64, a: f64) {
    if a.total_cmp(m).is_gt() {
        *m = a;
    }
}

/// The overlap fraction (pinned rule in the [module documentation](self)), counted on
/// `threads` threads (`1..=MAX_THREADS`); the result does not depend on `threads`.
pub fn overlap_fraction_with(
    moving: &PointCloud,
    pose: &Rigid,
    target: &OrientedCloudTarget,
    d: f64,
    max_samples: u64,
    threads: usize,
) -> Result<Overlap, RegError> {
    require_same_frame_ref(moving.frame(), target.frame()).map_err(RegError::FrameMismatch)?;
    if !(d.is_finite() && d > 0.0) {
        return Err(RegError::invalid("d", format!("the overlap distance must be finite and > 0, not {d}")));
    }
    if max_samples == 0 {
        return Err(RegError::invalid("max_samples", "the sample size must be at least 1".to_string()));
    }
    if threads == 0 || threads > MAX_THREADS {
        return Err(RegError::invalid("threads", format!("{threads} is not in 1..={MAX_THREADS}")));
    }
    if moving.is_empty() {
        return Err(RegError::invalid(
            "moving",
            "the moving cloud has no points, so no fraction is defined".to_string(),
        ));
    }
    let sample = stride_sample(moving.len(), max_samples)?;
    let sampled = sample.sample_len();
    let xf = Transform::Rigid(*pose);

    let mut scale = target.coord_scale();
    bigger(&mut scale, max_abs(&pose.t()));
    for i in sample.clone() {
        let (x, y) = image(moving, &xf, i)?;
        bigger(&mut scale, max_abs(&x));
        bigger(&mut scale, max_abs(&y));
    }
    let floor = FLOOR_FACTOR * EPS * scale;
    if d < floor {
        return Err(RegError::invalid(
            "d",
            format!("{d} m is below the rounding floor {floor} m (64 * 2^-53 * {scale} m) of these coordinates"),
        ));
    }

    let count_range = |from: u64, to: u64| -> RangeCount {
        let mut within = 0u64;
        for pos in from..to {
            let i = sample.index(pos).ok_or((pos, RegError::Inconsistent))?;
            let (_, y) = image(moving, &xf, i).map_err(|e| (pos, e))?;
            let near = target.nearest(y).map_err(|e| (pos, RegError::Index(e)))?;
            let dist = fid_num::f::sqrt(near.dist2).map_err(|_| (pos, RegError::Inconsistent))?;
            if dist <= d {
                within += 1;
            }
        }
        Ok(within)
    };

    let workers = u64::try_from(threads).unwrap_or(1);
    let per = sampled.div_ceil(workers);
    let results: Mutex<Vec<(u64, RangeCount)>> = Mutex::new(Vec::new());
    let failed = AtomicBool::new(false);
    let run = |w: u64| {
        let from = w.saturating_mul(per).min(sampled);
        let to = from.saturating_add(per).min(sampled);
        let r = count_range(from, to);
        match results.lock() {
            Ok(mut g) => g.push((w, r)),
            Err(_) => failed.store(true, Ordering::Release),
        }
    };
    std::thread::scope(|s| {
        let mut handles = Vec::new();
        let mut local: Vec<u64> = Vec::new();
        for w in 1..workers {
            // A thread that cannot be started is not an error: the calling thread takes its range.
            match std::thread::Builder::new().spawn_scoped(s, move || run(w)) {
                Ok(h) => handles.push(h),
                Err(_) => local.push(w),
            }
        }
        run(0);
        for w in local {
            run(w);
        }
        for h in handles {
            if h.join().is_err() {
                failed.store(true, Ordering::Release);
            }
        }
    });
    if failed.load(Ordering::Acquire) {
        return Err(RegError::Inconsistent);
    }
    let mut parts = results.into_inner().map_err(|_| RegError::Inconsistent)?;
    if u64::try_from(parts.len()).ok() != Some(workers) {
        return Err(RegError::Inconsistent);
    }
    parts.sort_by_key(|(w, _)| *w);
    let mut within = 0u64;
    let mut first_error: Option<(u64, RegError)> = None;
    for (_, r) in parts {
        match r {
            Ok(c) => within += c,
            Err((pos, e)) => {
                if first_error.as_ref().map_or(true, |(p, _)| pos < *p) {
                    first_error = Some((pos, e));
                }
            }
        }
    }
    if let Some((_, e)) = first_error {
        return Err(e);
    }
    Ok(Overlap { within, sampled, fraction: within as f64 / sampled as f64 })
}
