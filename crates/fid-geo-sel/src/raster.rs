//! CPU scanline depth buffer for `ResolveMode::Visible` (spec node `GEO.SEL.RESOLVE`).
//!
//! Pure Rust, `f64`, no I/O, no `unsafe`, no `mul_add` (KNP-1, D0).
//!
//! # Pinned rasterisation rules
//!
//! - **Viewport:** `width x height` pixels (`width > 0`, `height > 0`, `width * height <=
//!   MAX_VIEWPORT_PIXELS`). Pixel `(col, row)` covers `[col, col + 1) x [row, row + 1)` with
//!   centre `(col + 0.5, row + 0.5)`.
//! - **Near/far clipping:** each camera-space triangle `(c0, c1, c2)` is clipped by
//!   Sutherland-Hodgman against forward depth `d = -z >= near` and then `d = -z <= far`
//!   into a stack polygon of at most 5 vertices, fan-triangulated as `(p[0], p[k], p[k+1])`.
//! - **Scanline fill:** each clipped sub-triangle is projected to screen-space vertices
//!   `(sx, sy, depth)`. Signed twice-area is
//!   `area2 = (sx1 - sx0) * (sy2 - sy0) - (sy1 - sy0) * (sx2 - sx0)`. Degenerate triangles
//!   (`area2 == 0.0` or non-finite) write no pixels; negatively oriented triangles swap
//!   vertices 1 and 2 so `area2 > 0.0` (two-sided rasterisation).
//! - **Pixel-centre test:** for every integer `(col, row)` in the clamped screen bounding box
//!   whose centre `(px, py) = (col + 0.5, row + 0.5)` satisfies all three edge functions:
//!   - `w0 = (sx2 - sx1) * (py - sy1) - (sy2 - sy1) * (px - sx1) >= 0.0`
//!   - `w1 = (sx0 - sx2) * (py - sy2) - (sy0 - sy2) * (px - sx2) >= 0.0`
//!   - `w2 = (sx1 - sx0) * (py - sy0) - (sy1 - sy0) * (px - sx0) >= 0.0`
//!
//!   Barycentric coordinates are `l0 = w0 / area2`, `l1 = w1 / area2`, `l2 = w2 / area2`.
//! - **Depth interpolation:**
//!   - Perspective: `inv_d = (l0 / d0 + l1 / d1) + l2 / d2`, `d = 1.0 / inv_d`.
//!   - Orthographic: `d = (l0 * d0 + l1 * d1) + l2 * d2`.
//! - **Depth update:** `depth[row * width + col]` starts at `+INFINITY` and is replaced by `d`
//!   iff `d.total_cmp(&cur) == Ordering::Less` (fixed triangle order, total-order compare).
//! - **Visibility test (REQ-GEO.SEL.RESOLVE-004):** a projected point `(sx, sy, d)` is visible
//!   iff `(sx, sy)` falls in `[0, width) x [0, height)` and at its pixel `(floor(sx), floor(sy))`,
//!   either `buf_d == +INFINITY` or `d.total_cmp(&(buf_d + depth_tol)) != Ordering::Greater`.

use crate::model::SelError;
use core::cmp::Ordering;

#[inline]
fn tmin(a: f64, b: f64) -> f64 {
    if a.total_cmp(&b) == Ordering::Greater {
        b
    } else {
        a
    }
}

#[inline]
fn tmax(a: f64, b: f64) -> f64 {
    if a.total_cmp(&b) == Ordering::Less {
        b
    } else {
        a
    }
}

/// Largest accepted viewport pixel count (`width * height`) for CPU depth-buffer allocation
/// (64 M pixels = 512 MiB of `f64` depths).
pub const MAX_VIEWPORT_PIXELS: u64 = 64 * 1024 * 1024;

/// CPU depth buffer at viewport resolution storing forward camera-space depth `d = -z_cam`.
#[derive(Debug, Clone)]
pub struct DepthBuffer {
    width: u32,
    height: u32,
    depth: Vec<f64>,
}

impl DepthBuffer {
    /// Allocates a new depth buffer of `width x height` pixels initialised to `+INFINITY`.
    pub fn new(width: u32, height: u32) -> Result<DepthBuffer, SelError> {
        if width == 0 || height == 0 {
            return Err(SelError::BadCamera { reason: "viewport width and height must be positive" });
        }
        let pixels = u64::from(width).saturating_mul(u64::from(height));
        if pixels > MAX_VIEWPORT_PIXELS {
            return Err(SelError::AllocationFailed);
        }
        let n = usize::try_from(pixels).map_err(|_| SelError::AllocationFailed)?;
        let mut depth = Vec::new();
        depth.try_reserve_exact(n).map_err(|_| SelError::AllocationFailed)?;
        depth.resize(n, f64::INFINITY);
        Ok(DepthBuffer { width, height, depth })
    }

    /// Viewport width in pixels.
    pub const fn width(&self) -> u32 {
        self.width
    }

    /// Viewport height in pixels.
    pub const fn height(&self) -> u32 {
        self.height
    }

    /// Raw depth slice, row-major (`row * width + col`).
    pub fn as_slice(&self) -> &[f64] {
        &self.depth
    }

    /// Maps screen coordinates `(sx, sy)` in `[0, width) x [0, height)` to a flat buffer index.
    pub fn pixel_index(&self, sx: f64, sy: f64) -> Option<usize> {
        if !sx.is_finite() || !sy.is_finite() {
            return None;
        }
        let wf = f64::from(self.width);
        let hf = f64::from(self.height);
        if sx < 0.0 || sx >= wf || sy < 0.0 || sy >= hf {
            return None;
        }
        let col = sx.floor() as u32;
        let row = sy.floor() as u32;
        if col >= self.width || row >= self.height {
            return None;
        }
        let idx = u64::from(row).checked_mul(u64::from(self.width))?.checked_add(u64::from(col))?;
        usize::try_from(idx).ok()
    }

    /// Writes `d` into pixel `idx` if `d` is strictly nearer in IEEE-754 totalOrder.
    #[inline]
    pub fn write_min(&mut self, idx: usize, d: f64) {
        if !d.is_finite() {
            return;
        }
        if let Some(slot) = self.depth.get_mut(idx) {
            if d.total_cmp(slot) == Ordering::Less {
                *slot = d;
            }
        }
    }

    /// Splats a single projected point `(sx, sy, d)` into its pixel (used for point sets/clouds
    /// in `Visible` mode).
    pub fn splat_point(&mut self, sx: f64, sy: f64, d: f64) {
        if let Some(idx) = self.pixel_index(sx, sy) {
            self.write_min(idx, d);
        }
    }

    /// Tests whether a projected point `(sx, sy, d)` is visible against the depth buffer within
    /// `depth_tol` (REQ-GEO.SEL.RESOLVE-004).
    pub fn is_visible(&self, sx: f64, sy: f64, d: f64, depth_tol: f64) -> bool {
        if !d.is_finite() || !depth_tol.is_finite() || depth_tol < 0.0 {
            return false;
        }
        let Some(idx) = self.pixel_index(sx, sy) else {
            return false;
        };
        let Some(&buf_d) = self.depth.get(idx) else {
            return false;
        };
        if buf_d.is_infinite() {
            return buf_d.is_sign_positive();
        }
        let limit = buf_d + depth_tol;
        d.total_cmp(&limit) != Ordering::Greater
    }

    /// Scanline-rasterises one already-clipped screen-space triangle `([sx, sy, d] x 3)` into the
    /// depth buffer at pixel centres `(col + 0.5, row + 0.5)`.
    pub fn rasterise_screen_triangle(&mut self, v0: [f64; 3], mut v1: [f64; 3], mut v2: [f64; 3], perspective: bool) {
        for v in [&v0, &v1, &v2] {
            if !v[0].is_finite() || !v[1].is_finite() || !v[2].is_finite() {
                return;
            }
            if perspective && v[2] <= 0.0 {
                return;
            }
        }
        let mut area2 = (v1[0] - v0[0]) * (v2[1] - v0[1]) - (v1[1] - v0[1]) * (v2[0] - v0[0]);
        if !area2.is_finite() || area2 == 0.0 {
            return;
        }
        if area2 < 0.0 {
            core::mem::swap(&mut v1, &mut v2);
            area2 = -area2;
        }

        // Exact triangle screen bounding box in IEEE-754 totalOrder (all coordinates are finite).
        let min_x = tmin(tmin(v0[0], v1[0]), v2[0]);
        let max_x = tmax(tmax(v0[0], v1[0]), v2[0]);
        let min_y = tmin(tmin(v0[1], v1[1]), v2[1]);
        let max_y = tmax(tmax(v0[1], v1[1]), v2[1]);

        let wf = f64::from(self.width);
        let hf = f64::from(self.height);
        if max_x < 0.5 || min_x > wf - 0.5 || max_y < 0.5 || min_y > hf - 0.5 {
            return;
        }

        let col_start = clamp_pixel_lo(min_x - 0.5, self.width);
        let col_end = clamp_pixel_hi(max_x - 0.5, self.width);
        let row_start = clamp_pixel_lo(min_y - 0.5, self.height);
        let row_end = clamp_pixel_hi(max_y - 0.5, self.height);
        if col_start > col_end || row_start > row_end {
            return;
        }

        let (sx0, sy0, d0) = (v0[0], v0[1], v0[2]);
        let (sx1, sy1, d1) = (v1[0], v1[1], v1[2]);
        let (sx2, sy2, d2) = (v2[0], v2[1], v2[2]);

        let e0x = sx2 - sx1;
        let e0y = sy2 - sy1;
        let e1x = sx0 - sx2;
        let e1y = sy0 - sy2;
        let e2x = sx1 - sx0;
        let e2y = sy1 - sy0;

        let w_usize = self.width as usize;
        for row in row_start..=row_end {
            let py = f64::from(row) + 0.5;
            let row_base = (row as usize).saturating_mul(w_usize);
            for col in col_start..=col_end {
                let px = f64::from(col) + 0.5;
                let w0 = e0x * (py - sy1) - e0y * (px - sx1);
                if w0 < 0.0 {
                    continue;
                }
                let w1 = e1x * (py - sy2) - e1y * (px - sx2);
                if w1 < 0.0 {
                    continue;
                }
                let w2 = e2x * (py - sy0) - e2y * (px - sx0);
                if w2 < 0.0 {
                    continue;
                }
                let l0 = w0 / area2;
                let l1 = w1 / area2;
                let l2 = w2 / area2;
                let d = if perspective {
                    let inv_d = (l0 / d0 + l1 / d1) + l2 / d2;
                    if !inv_d.is_finite() || inv_d <= 0.0 {
                        continue;
                    }
                    1.0 / inv_d
                } else {
                    (l0 * d0 + l1 * d1) + l2 * d2
                };
                self.write_min(row_base + (col as usize), d);
            }
        }
    }
}

fn clamp_pixel_lo(v: f64, dim: u32) -> u32 {
    if v <= 0.0 {
        0
    } else {
        let c = v.ceil();
        let max_idx = f64::from(dim - 1);
        if c >= max_idx {
            dim - 1
        } else {
            c as u32
        }
    }
}

fn clamp_pixel_hi(v: f64, dim: u32) -> u32 {
    if v < 0.0 {
        // Return a sentinel? No: caller checks `max_x < 0.5` before calling, so `v >= 0.0`.
        0
    } else {
        let f = v.floor();
        let max_idx = f64::from(dim - 1);
        if f >= max_idx {
            dim - 1
        } else {
            f as u32
        }
    }
}

/// Clips a camera-space triangle `(c0, c1, c2)` against forward depth `near <= -z <= far`
/// using Sutherland-Hodgman into a stack buffer of at most 5 vertices.
///
/// Returns `(verts, count)`. When `count < 3`, the triangle lies outside `[near, far]`.
/// Fan-triangulating `(verts[0], verts[k], verts[k + 1])` for `k in 1..count - 1` yields at
/// most 3 sub-triangles whose vertices all satisfy `near <= -z <= far`.
pub fn clip_triangle_depth(c0: [f64; 3], c1: [f64; 3], c2: [f64; 3], near: f64, far: f64) -> ([[f64; 3]; 5], usize) {
    let mut buf_a = [[0.0; 3]; 5];
    buf_a[0] = c0;
    buf_a[1] = c1;
    buf_a[2] = c2;
    let (buf_b, n_near) = clip_plane(&buf_a, 3, near, true);
    if n_near < 3 {
        return (buf_b, 0);
    }
    let (buf_c, n_far) = clip_plane(&buf_b, n_near, far, false);
    if n_far < 3 {
        return (buf_c, 0);
    }
    (buf_c, n_far)
}

/// Clips a convex polygon (<= 4 input vertices) against `d >= plane` (when `keep_ge == true`)
/// or `d <= plane` (when `keep_ge == false`), where `d = -v[2]`.
fn clip_plane(poly: &[[f64; 3]; 5], count: usize, plane: f64, keep_ge: bool) -> ([[f64; 3]; 5], usize) {
    let mut out = [[0.0; 3]; 5];
    let mut out_n = 0usize;
    if count == 0 {
        return (out, 0);
    }
    let inside = |v: [f64; 3]| -> bool {
        let d = -v[2];
        if keep_ge {
            d >= plane
        } else {
            d <= plane
        }
    };
    let mut prev = poly[count - 1];
    let mut prev_in = inside(prev);
    for &cur in poly.iter().take(count) {
        let cur_in = inside(cur);
        if cur_in != prev_in {
            let da = -prev[2];
            let db = -cur[2];
            let denom = db - da;
            if denom != 0.0 && denom.is_finite() {
                let t = (plane - da) / denom;
                let ix = prev[0] + t * (cur[0] - prev[0]);
                let iy = prev[1] + t * (cur[1] - prev[1]);
                let iz = -plane;
                if let Some(slot) = out.get_mut(out_n) {
                    *slot = [ix, iy, iz];
                    out_n += 1;
                }
            }
        }
        if cur_in {
            if let Some(slot) = out.get_mut(out_n) {
                *slot = cur;
                out_n += 1;
            }
        }
        prev = cur;
        prev_in = cur_in;
    }
    (out, out_n)
}
