//! `fid-design glyph-distinct --root <icons dir> [--threshold 0.20] [--json]` (spec node
//! `UX.ICONS.CLASS`, REQ-UX.ICONS.CLASS-001 .. -004). Tooling, nearest class R0; D0 (integer
//! coverage after a fixed supersampling, sorted findings, no clock, no network).
//!
//! # What is checked
//!
//! The meaning-carrying glyphs come in three groups: the eleven side-effect class icons
//! ([`CLASSES`]), the six display-state glyphs ([`STATES`]) and the five provenance badges
//! ([`BADGES`]). For each name the root must hold the 16 px master `svg/<name>.16.svg` and the
//! icon index `index.json` must hold an entry with `a11y_id` = `icon.<name>`; otherwise
//! `GLYPH-MISSING <name>` (REQ-001).
//!
//! # Rasteriser
//!
//! Each 16 px master must first pass the icon SVG profile of `UX.ICONS.GRID`
//! ([`crate::icons_lint::lint_svg_str`]); then it is rasterised at 16 px greyscale with 4x
//! supersampling and anti-aliasing off: one sample at the centre of each quarter-pixel cell
//! (64 x 64 samples), a pixel covered when at least 8 of its 16 samples are (coverage threshold
//! 0.5). Every painted shape counts as covered (single-tone `currentColor`); `fill` and `stroke`
//! are honoured with `stroke-width`, `stroke-linecap` (butt, round, square) and `fill-rule`
//! (nonzero, evenodd). Stroke joins are rasterised as round, whatever `stroke-linejoin` says,
//! except the corners of a sharp `rect` with a miter join (square corners). Circles and
//! rectangles with equal corner radii are decided analytically; straight segments with the
//! squared-distance test, which is exact for coordinates on the 0.5 px grid. Curves are
//! flattened: cubic and quadratic Beziers into 16 uniform steps, arcs by bisection into 64
//! steps (elliptical arcs only when their axes are horizontal and vertical). Anything else (a rotated elliptical arc, unequal `rx`/`ry` on a `rect`, an
//! opacity below 1, more than [`MAX_POINTS`] flattened points) or a glyph that covers no pixel
//! is `GLYPH-RASTER <name>` and takes no part in the comparisons.
//!
//! # Metric (REQ-002)
//!
//! For each pair in a group: the symmetric difference of covered pixels divided by their union,
//! minimised over integer shifts of the second glyph in -1..1 px on both axes. A pair below the
//! threshold (default 0.20, compared as exact fractions) is `GLYPH-SIMILAR <a> <b>`.
//!
//! # Shape families (REQ-003, REQ-004)
//!
//! The outer shape of a glyph is its silhouette: its covered pixels plus every pixel the
//! background cannot reach from the canvas border through 4-connected uncovered pixels. With
//! the same metric on silhouettes:
//!
//! - the W3, M1, S1 and A0 class icons ([`FAMILY_MEMBERS`]) must share an outer shape: every
//!   pair differs by at most [`FAMILY_SAME`] (1/10); else `GLYPH-FAMILY <a> <b>`;
//! - no other class icon may use it: each differs from every family member by at least the
//!   threshold; else `GLYPH-FAMILY <other> <member>`;
//! - the stale and not-evaluated glyphs differ from the conforms and nonconforms glyphs by at
//!   least the threshold; else `GLYPH-FAMILY <a> <b>`.
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable root or index, or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::icons_lint::{self, PathCursor};
use crate::icons_prov::{parse_index, IndexEntry};
use crate::review::{self, Kind};
use fid_canon::CValue;
use std::collections::BTreeMap;
use std::path::Path;

/// Finding codes (UX.ICONS.CLASS Design).
pub const MISSING: &str = "GLYPH-MISSING";
pub const SIMILAR: &str = "GLYPH-SIMILAR";
pub const FAMILY: &str = "GLYPH-FAMILY";
pub const RASTER: &str = "GLYPH-RASTER";

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-glyph-distinct@1";
/// The default `--threshold` (REQ-UX.ICONS.CLASS-002, an unverified threshold).
pub const DEFAULT_THRESHOLD: &str = "0.20";
/// Master size in pixels.
pub const SIZE: usize = 16;
/// Samples per pixel along each axis.
pub const SUPERSAMPLE: usize = 4;
/// Samples (of 16) that make a pixel covered: coverage threshold 0.5.
pub const COVER_MIN: u32 = 8;
/// Uniform steps per Bezier segment.
pub const CURVE_STEPS: u32 = 16;
/// Bisection depth of each quarter of an arc (2^4 = 16 steps, 64 per arc).
pub const ARC_DEPTH: u32 = 4;
/// Most flattened points in one glyph.
pub const MAX_POINTS: usize = 8192;
/// The icon index, below the root.
pub const INDEX_FILE: &str = "index.json";
/// Largest icon index read.
pub const MAX_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// The side-effect class icons (CONSTITUTION §3).
pub const CLASSES: [&str; 11] = [
    "class.r0", "class.r1", "class.w1", "class.w2", "class.w3", "class.x1", "class.i1", "class.d1", "class.m1",
    "class.s1", "class.a0",
];
/// The display-state glyphs.
pub const STATES: [&str; 6] = [
    "state.conforms",
    "state.nonconforms",
    "state.indeterminate",
    "state.not-decidable",
    "state.not-evaluated",
    "state.stale",
];
/// The provenance badges.
pub const BADGES: [&str; 5] = ["prov.computed", "prov.measured", "prov.entered", "prov.reported", "prov.suggested"];
/// The groups compared pairwise: group ID and its glyph names.
pub const GROUPS: [(&str, &[&str]); 3] = [("class", &CLASSES), ("state", &STATES), ("prov", &BADGES)];
/// Human-only and never-on-MCP classes that share an outer shape (REQ-003).
pub const FAMILY_MEMBERS: [&str; 4] = ["class.w3", "class.m1", "class.s1", "class.a0"];
/// Most a family member's silhouette may differ from another's (1/10, an unverified value).
pub const FAMILY_SAME: (u64, u64) = (1, 10);
/// Glyphs whose outer shape must differ (REQ-004): each of the first from each of the second.
pub const STATES_APART: ([&str; 2], [&str; 2]) =
    (["state.stale", "state.not-evaluated"], ["state.conforms", "state.nonconforms"]);

/// Covered pixels, one row per `u16` (bit `x` is column `x`).
pub type Bitmap = [u16; SIZE];

/// A `--threshold` value: an exact fraction in (0, 1].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Threshold {
    pub text: String,
    pub num: u64,
    pub den: u64,
}

impl Threshold {
    /// Parses a decimal such as `0.20`, `.25` or `1` (at most 6 decimals, in (0, 1]).
    pub fn parse(s: &str) -> Result<Self, String> {
        let err = || format!("--threshold {s:?} must be a decimal in (0, 1] with at most 6 decimals");
        let (int, frac) = match s.split_once('.') {
            Some((i, f)) if !f.is_empty() => (i, f),
            Some(_) => return Err(err()),
            None => (s, ""),
        };
        let digits = |t: &str| t.bytes().all(|b| b.is_ascii_digit());
        if int.len() > 1 || frac.len() > 6 || !digits(int) || !digits(frac) || (int.is_empty() && frac.is_empty()) {
            return Err(err());
        }
        let mut num: u64 = 0;
        let mut den: u64 = 1;
        for b in int.bytes() {
            num = num * 10 + u64::from(b - b'0');
        }
        for b in frac.bytes() {
            num = num * 10 + u64::from(b - b'0');
            den *= 10;
        }
        if num == 0 || num > den {
            return Err(err());
        }
        Ok(Threshold { text: s.to_string(), num, den })
    }

    /// The default threshold, 0.20.
    pub fn default_value() -> Self {
        Threshold { text: DEFAULT_THRESHOLD.to_string(), num: 20, den: 100 }
    }

    /// Whether `d` is below the threshold.
    pub fn below(&self, d: &Diff) -> bool {
        u128::from(d.symdiff) * u128::from(self.den) < u128::from(self.num) * u128::from(d.union)
    }
}

/// The difference of two bitmaps at one shift of the second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    pub symdiff: u64,
    pub union: u64,
    pub dx: i32,
    pub dy: i32,
}

impl Diff {
    /// Whether this ratio is strictly smaller than `o`'s.
    pub fn less(&self, o: &Diff) -> bool {
        self.symdiff * o.union < o.symdiff * self.union
    }

    /// The ratio in parts per million, rounded down.
    pub fn ratio_ppm(&self) -> u64 {
        self.symdiff.saturating_mul(1_000_000).checked_div(self.union).unwrap_or(0)
    }
}

fn ratio_text(ppm: u64) -> String {
    format!("{}.{:06}", ppm / 1_000_000, ppm % 1_000_000)
}

/// `b` moved by `dx` columns and `dy` rows; pixels moved off the canvas are lost.
pub fn shifted(b: &Bitmap, dx: i32, dy: i32) -> Bitmap {
    let mut out = [0u16; SIZE];
    for (y, o) in out.iter_mut().enumerate() {
        let sy = y as i64 - i64::from(dy);
        let r = if (0..SIZE as i64).contains(&sy) { b.get(sy as usize).copied().unwrap_or(0) } else { 0 };
        *o = if dx >= 0 {
            r.checked_shl(dx.unsigned_abs()).unwrap_or(0)
        } else {
            r.checked_shr(dx.unsigned_abs()).unwrap_or(0)
        };
    }
    out
}

/// Number of covered pixels.
pub fn count(b: &Bitmap) -> u64 {
    b.iter().map(|r| u64::from(r.count_ones())).sum()
}

/// The smallest symmetric-difference-over-union of `a` and `b` over shifts of `b` in -1..1 px
/// (rows outer, columns inner, first minimum kept); `None` when both are empty.
pub fn best_diff(a: &Bitmap, b: &Bitmap) -> Option<Diff> {
    let mut res: Option<Diff> = None;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let s = shifted(b, dx, dy);
            let mut sd = 0u64;
            let mut un = 0u64;
            for (ra, rs) in a.iter().zip(s.iter()) {
                sd += u64::from((ra ^ rs).count_ones());
                un += u64::from((ra | rs).count_ones());
            }
            if un == 0 {
                continue;
            }
            let d = Diff { symdiff: sd, union: un, dx, dy };
            if res.map_or(true, |r| d.less(&r)) {
                res = Some(d);
            }
        }
    }
    res
}

/// The covered pixels plus every pixel the background cannot reach from the canvas border
/// through 4-connected uncovered pixels.
pub fn silhouette(b: &Bitmap) -> Bitmap {
    let covered = |x: usize, y: usize| b.get(y).is_some_and(|r| (r >> x) & 1 == 1);
    let mut seen = [[false; SIZE]; SIZE];
    let mut stack: Vec<(usize, usize)> = Vec::new();
    for i in 0..SIZE {
        for (x, y) in [(i, 0), (i, SIZE - 1), (0, i), (SIZE - 1, i)] {
            if !covered(x, y) && !seen[y][x] {
                seen[y][x] = true;
                stack.push((x, y));
            }
        }
    }
    while let Some((x, y)) = stack.pop() {
        let mut next = Vec::with_capacity(4);
        if x + 1 < SIZE {
            next.push((x + 1, y));
        }
        if x > 0 {
            next.push((x - 1, y));
        }
        if y + 1 < SIZE {
            next.push((x, y + 1));
        }
        if y > 0 {
            next.push((x, y - 1));
        }
        for (nx, ny) in next {
            if !seen[ny][nx] && !covered(nx, ny) {
                seen[ny][nx] = true;
                stack.push((nx, ny));
            }
        }
    }
    let mut out = [0u16; SIZE];
    for (y, o) in out.iter_mut().enumerate() {
        for (x, s) in seen[y].iter().enumerate() {
            if !s {
                *o |= 1 << x;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Rasteriser
// ---------------------------------------------------------------------------------------------

type Pt = (f64, f64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Join {
    Miter,
    Other,
}

#[derive(Debug, Clone, Copy)]
struct Paint {
    fill: bool,
    stroke: bool,
    width: f64,
    cap: Cap,
    join: Join,
    evenodd: bool,
}

impl Paint {
    /// SVG initial values: filled, not stroked, width 1, butt caps, miter joins, nonzero.
    fn initial() -> Self {
        Paint { fill: true, stroke: false, width: 1.0, cap: Cap::Butt, join: Join::Miter, evenodd: false }
    }
}

#[derive(Debug, Clone)]
struct Sub {
    pts: Vec<Pt>,
    closed: bool,
}

#[derive(Debug, Clone)]
enum Geom {
    Circle { cx: f64, cy: f64, r: f64 },
    Rect { cx: f64, cy: f64, hx: f64, hy: f64, rho: f64 },
    Poly { subs: Vec<Sub>, line: bool },
}

#[derive(Debug, Clone)]
struct Shape {
    geom: Geom,
    paint: Paint,
}

fn number(s: &str) -> Option<f64> {
    let t = s.trim();
    let t = t.strip_suffix("px").unwrap_or(t);
    let v: f64 = t.trim().parse().ok()?;
    v.is_finite().then_some(v)
}

fn attr<'a>(attrs: &'a [(String, String)], key: &str) -> Option<&'a str> {
    attrs.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

fn num_attr(attrs: &[(String, String)], key: &str, default: Option<f64>) -> Result<f64, String> {
    match attr(attrs, key) {
        Some(v) => number(v).ok_or_else(|| format!("`{key}` {v:?} is not a finite number")),
        None => default.ok_or_else(|| format!("`{key}` is missing")),
    }
}

fn apply_paint(p: &mut Paint, attrs: &[(String, String)]) -> Result<(), String> {
    for (k, v) in attrs {
        match k.as_str() {
            "fill" => p.fill = v != "none",
            "stroke" => p.stroke = v != "none",
            "stroke-width" => match number(v) {
                Some(w) if w >= 0.0 => p.width = w,
                _ => return Err(format!("`stroke-width` {v:?} is not a non-negative number")),
            },
            "stroke-linecap" => {
                p.cap = match v.as_str() {
                    "butt" => Cap::Butt,
                    "round" => Cap::Round,
                    "square" => Cap::Square,
                    _ => return Err(format!("`stroke-linecap` {v:?} is not supported")),
                }
            }
            "stroke-linejoin" => {
                p.join = match v.as_str() {
                    "miter" | "miter-clip" => Join::Miter,
                    "round" | "bevel" | "arcs" => Join::Other,
                    _ => return Err(format!("`stroke-linejoin` {v:?} is not supported")),
                }
            }
            "fill-rule" => {
                p.evenodd = match v.as_str() {
                    "nonzero" => false,
                    "evenodd" => true,
                    _ => return Err(format!("`fill-rule` {v:?} is not supported")),
                }
            }
            "opacity" | "fill-opacity" | "stroke-opacity" => {
                if number(v) != Some(1.0) {
                    return Err(format!("`{k}` {v:?}: a meaning-carrying glyph is drawn at full opacity"));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

struct Budget(usize);

impl Budget {
    fn take(&mut self, n: usize) -> Result<(), String> {
        self.0 = self.0.saturating_add(n);
        if self.0 > MAX_POINTS {
            Err(format!("more than {MAX_POINTS} flattened points"))
        } else {
            Ok(())
        }
    }
}

fn read_pair(cur: &mut PathCursor<'_>, what: &str) -> Result<Pt, String> {
    let x = cur.read_number().ok_or_else(|| format!("missing x in {what}"))?;
    let y = cur.read_number().ok_or_else(|| format!("missing y in {what}"))?;
    Ok((x, y))
}

fn cubic(p0: Pt, c1: Pt, c2: Pt, p1: Pt, out: &mut Vec<Pt>) {
    for k in 1..=CURVE_STEPS {
        let t = f64::from(k) / f64::from(CURVE_STEPS);
        let mt = 1.0 - t;
        let a = mt * mt * mt;
        let b = 3.0 * mt * mt * t;
        let c = 3.0 * mt * t * t;
        let d = t * t * t;
        out.push((a * p0.0 + b * c1.0 + c * c2.0 + d * p1.0, a * p0.1 + b * c1.1 + c * c2.1 + d * p1.1));
    }
}

fn quad(p0: Pt, c: Pt, p1: Pt, out: &mut Vec<Pt>) {
    for k in 1..=CURVE_STEPS {
        let t = f64::from(k) / f64::from(CURVE_STEPS);
        let mt = 1.0 - t;
        let a = mt * mt;
        let b = 2.0 * mt * t;
        let d = t * t;
        out.push((a * p0.0 + b * c.0 + d * p1.0, a * p0.1 + b * c.1 + d * p1.1));
    }
}

fn unit(v: Pt) -> Option<Pt> {
    let l = (v.0 * v.0 + v.1 * v.1).sqrt();
    if l > 0.0 && l.is_finite() {
        Some((v.0 / l, v.1 / l))
    } else {
        None
    }
}

/// Unit directions from `u` to `v` (both unit, less than half a turn apart) by bisection.
fn bisect(u: Pt, v: Pt, depth: u32, out: &mut Vec<Pt>) {
    if depth == 0 {
        out.push(v);
        return;
    }
    match unit((u.0 + v.0, u.1 + v.1)) {
        Some(m) => {
            bisect(u, m, depth - 1, out);
            bisect(m, v, depth - 1, out);
        }
        None => out.push(v),
    }
}

/// Points of an SVG arc from `p0` to `p1` (excluding `p0`), endpoint parameterisation per SVG
/// 1.1 appendix F.6 with the axes horizontal and vertical.
#[allow(clippy::too_many_arguments)]
fn arc(p0: Pt, rx: f64, ry: f64, rot: f64, large: bool, sweep: bool, p1: Pt, out: &mut Vec<Pt>) -> Result<(), String> {
    if p0 == p1 {
        return Ok(());
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx == 0.0 || ry == 0.0 {
        out.push(p1);
        return Ok(());
    }
    let turn = rot.rem_euclid(180.0);
    if turn == 90.0 {
        std::mem::swap(&mut rx, &mut ry);
    } else if turn != 0.0 && rx != ry {
        return Err(format!("an elliptical arc rotated by {rot} degrees is not supported"));
    }
    // Map the ellipse to a circle of radius rx by scaling y.
    let s = rx / ry;
    let a = (p0.0, p0.1 * s);
    let b = (p1.0, p1.1 * s);
    let h = ((a.0 - b.0) / 2.0, (a.1 - b.1) / 2.0);
    let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
    let d2 = h.0 * h.0 + h.1 * h.1;
    let mut r = rx;
    let coef = if r * r <= d2 {
        r = d2.sqrt();
        0.0
    } else {
        let c = ((r * r - d2) / d2).sqrt();
        if large == sweep {
            -c
        } else {
            c
        }
    };
    let c = (mid.0 + coef * h.1, mid.1 - coef * h.0);
    let (Some(u), Some(v)) = (unit((a.0 - c.0, a.1 - c.1)), unit((b.0 - c.0, b.1 - c.1))) else {
        out.push(p1);
        return Ok(());
    };
    let cross = u.0 * v.1 - u.1 * v.0;
    let dot = u.0 * v.0 + u.1 * v.1;
    // The direction half-way along the arc from u to v in the sweep direction.
    let positive = sweep;
    let m = if cross == 0.0 && dot < 0.0 {
        if positive {
            (-u.1, u.0)
        } else {
            (u.1, -u.0)
        }
    } else {
        let Some(n) = unit((u.0 + v.0, u.1 + v.1)) else {
            out.push(p1);
            return Ok(());
        };
        let minor = if positive { cross > 0.0 } else { cross < 0.0 };
        if minor {
            n
        } else {
            (-n.0, -n.1)
        }
    };
    let mut dirs = Vec::new();
    // Each half is at most half a turn; split it once more so every bisection starts below it.
    for (from, to) in [(u, m), (m, v)] {
        let q = unit((from.0 + to.0, from.1 + to.1)).unwrap_or(if positive {
            (-from.1, from.0)
        } else {
            (from.1, -from.0)
        });
        bisect(from, q, ARC_DEPTH, &mut dirs);
        bisect(q, to, ARC_DEPTH, &mut dirs);
    }
    let n = dirs.len();
    for (i, w) in dirs.into_iter().enumerate() {
        if i + 1 == n {
            out.push(p1);
        } else {
            out.push((c.0 + r * w.0, (c.1 + r * w.1) / s));
        }
    }
    Ok(())
}

fn parse_path(d: &str, budget: &mut Budget) -> Result<Vec<Sub>, String> {
    let mut cur = PathCursor::new(d);
    let mut subs: Vec<Sub> = Vec::new();
    let mut pts: Vec<Pt> = Vec::new();
    let (mut x, mut y, mut sx, mut sy) = (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let mut cmd: Option<u8> = None;
    // The last control point, for the reflections of `S` (after C/S) and `T` (after Q/T).
    let mut last_ctrl: Option<(u8, Pt)> = None;
    cur.skip_ws();
    while !cur.is_eof() {
        let Some(b) = cur.peek() else { break };
        let active = if b.is_ascii_alphabetic() {
            cur.pos += 1;
            cmd = Some(b);
            b
        } else {
            match cmd {
                Some(b'M') => b'L',
                Some(b'm') => b'l',
                Some(b'Z' | b'z') | None => return Err("path `d` has a number without a command".to_string()),
                Some(c) => c,
            }
        };
        let rel = active.is_ascii_lowercase();
        let at = |p: Pt| if rel { (x + p.0, y + p.1) } else { p };
        let mut ctrl: Option<(u8, Pt)> = None;
        let before = pts.len();
        let upper = active.to_ascii_uppercase();
        if upper != b'M' && upper != b'Z' && pts.is_empty() {
            pts.push((x, y));
        }
        match upper {
            b'M' => {
                let p = at(read_pair(&mut cur, "M/m")?);
                if !pts.is_empty() {
                    subs.push(Sub { pts: std::mem::take(&mut pts), closed: false });
                }
                (x, y) = p;
                (sx, sy) = p;
                pts.push(p);
                if cmd == Some(b'M') || cmd == Some(b'm') {
                    // Later pairs of the same command are implicit line-tos.
                    cmd = Some(if rel { b'l' } else { b'L' });
                }
            }
            b'Z' => {
                if !pts.is_empty() {
                    subs.push(Sub { pts: std::mem::take(&mut pts), closed: true });
                }
                (x, y) = (sx, sy);
            }
            b'L' => {
                let p = at(read_pair(&mut cur, "L/l")?);
                pts.push(p);
                (x, y) = p;
            }
            b'H' => {
                let v = cur.read_number().ok_or_else(|| "missing x in H/h".to_string())?;
                x = if rel { x + v } else { v };
                pts.push((x, y));
            }
            b'V' => {
                let v = cur.read_number().ok_or_else(|| "missing y in V/v".to_string())?;
                y = if rel { y + v } else { v };
                pts.push((x, y));
            }
            b'C' | b'S' => {
                let c1 = if upper == b'C' {
                    at(read_pair(&mut cur, "C/c")?)
                } else {
                    match last_ctrl {
                        Some((b'C', c)) => (2.0 * x - c.0, 2.0 * y - c.1),
                        _ => (x, y),
                    }
                };
                let c2 = at(read_pair(&mut cur, "C/S")?);
                let p = at(read_pair(&mut cur, "C/S")?);
                cubic((x, y), c1, c2, p, &mut pts);
                ctrl = Some((b'C', c2));
                (x, y) = p;
            }
            b'Q' | b'T' => {
                let c = if upper == b'Q' {
                    at(read_pair(&mut cur, "Q/q")?)
                } else {
                    match last_ctrl {
                        Some((b'Q', c)) => (2.0 * x - c.0, 2.0 * y - c.1),
                        _ => (x, y),
                    }
                };
                let p = at(read_pair(&mut cur, "Q/T")?);
                quad((x, y), c, p, &mut pts);
                ctrl = Some((b'Q', c));
                (x, y) = p;
            }
            b'A' => {
                let rx = cur.read_number().ok_or_else(|| "missing rx in A/a".to_string())?;
                let ry = cur.read_number().ok_or_else(|| "missing ry in A/a".to_string())?;
                let rot = cur.read_number().ok_or_else(|| "missing rotation in A/a".to_string())?;
                let large = cur.read_flag().ok_or_else(|| "invalid large-arc flag in A/a".to_string())?;
                let sweep = cur.read_flag().ok_or_else(|| "invalid sweep flag in A/a".to_string())?;
                let p = at(read_pair(&mut cur, "A/a")?);
                arc((x, y), rx, ry, rot, large == 1, sweep == 1, p, &mut pts)?;
                (x, y) = p;
            }
            other => return Err(format!("unknown path command `{}`", char::from(other))),
        }
        last_ctrl = ctrl;
        budget.take(pts.len().saturating_sub(before))?;
        cur.skip_comma_ws();
    }
    if !pts.is_empty() {
        subs.push(Sub { pts, closed: false });
    }
    Ok(subs)
}

fn shapes_of(text: &str) -> Result<Vec<Shape>, String> {
    let elems = icons_lint::svg_elements(text)?;
    let mut paints: Vec<Paint> = Vec::with_capacity(elems.len());
    let mut shapes = Vec::new();
    let mut budget = Budget(0);
    for (name, attrs, parent) in &elems {
        let mut p = parent.and_then(|i| paints.get(i).copied()).unwrap_or_else(Paint::initial);
        apply_paint(&mut p, attrs).map_err(|e| format!("<{name}>: {e}"))?;
        paints.push(p);
        let geom = match name.as_str() {
            "svg" | "g" => None,
            "circle" => {
                let r = num_attr(attrs, "r", None)?;
                Some(Geom::Circle { cx: num_attr(attrs, "cx", Some(0.0))?, cy: num_attr(attrs, "cy", Some(0.0))?, r })
            }
            "rect" => {
                let x = num_attr(attrs, "x", Some(0.0))?;
                let y = num_attr(attrs, "y", Some(0.0))?;
                let w = num_attr(attrs, "width", None)?;
                let h = num_attr(attrs, "height", None)?;
                let rx = attr(attrs, "rx").map(|_| num_attr(attrs, "rx", None)).transpose()?;
                let ry = attr(attrs, "ry").map(|_| num_attr(attrs, "ry", None)).transpose()?;
                let (rx, ry) = match (rx, ry) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) | (None, Some(a)) => (a, a),
                    (None, None) => (0.0, 0.0),
                };
                let (rx, ry) = (rx.min(w / 2.0), ry.min(h / 2.0));
                if rx != ry {
                    return Err("<rect>: unequal `rx` and `ry` (elliptical corners) are not supported".to_string());
                }
                Some(Geom::Rect { cx: x + w / 2.0, cy: y + h / 2.0, hx: w / 2.0, hy: h / 2.0, rho: rx })
            }
            "line" => {
                let a = (num_attr(attrs, "x1", Some(0.0))?, num_attr(attrs, "y1", Some(0.0))?);
                let b = (num_attr(attrs, "x2", Some(0.0))?, num_attr(attrs, "y2", Some(0.0))?);
                budget.take(2)?;
                Some(Geom::Poly { subs: vec![Sub { pts: vec![a, b], closed: false }], line: true })
            }
            "polyline" | "polygon" => {
                let pts = icons_lint::parse_points(attr(attrs, "points").unwrap_or(""))?;
                budget.take(pts.len())?;
                Some(Geom::Poly { subs: vec![Sub { pts, closed: name == "polygon" }], line: false })
            }
            "path" => Some(Geom::Poly { subs: parse_path(attr(attrs, "d").unwrap_or(""), &mut budget)?, line: false }),
            other => return Err(format!("<{other}> cannot be rasterised")),
        };
        if let Some(geom) = geom {
            shapes.push(Shape { geom, paint: p });
        }
    }
    Ok(shapes)
}

/// Inside the butt-ended stroke body of segment `a`-`b` (exact for dyadic inputs).
fn seg_body(px: f64, py: f64, a: Pt, b: Pt, hw: f64) -> bool {
    let dx = b.0 - a.0;
    let dy = b.1 - a.1;
    let dd = dx * dx + dy * dy;
    if dd == 0.0 {
        return false;
    }
    let ux = px - a.0;
    let uy = py - a.1;
    let s = ux * dx + uy * dy;
    if s < 0.0 || s > dd {
        return false;
    }
    let cr = ux * dy - uy * dx;
    cr * cr <= hw * hw * dd
}

fn disk(px: f64, py: f64, c: Pt, hw: f64) -> bool {
    let ux = px - c.0;
    let uy = py - c.1;
    ux * ux + uy * uy <= hw * hw
}

/// The square cap beyond `end` of the segment `other`-`end`.
fn square_cap(px: f64, py: f64, end: Pt, other: Pt, hw: f64) -> bool {
    let dx = end.0 - other.0;
    let dy = end.1 - other.1;
    let dd = dx * dx + dy * dy;
    if dd == 0.0 {
        return false;
    }
    let ln = dd.sqrt();
    let ux = px - end.0;
    let uy = py - end.1;
    let s = ux * dx + uy * dy;
    if s < 0.0 || s > hw * ln {
        return false;
    }
    let cr = ux * dy - uy * dx;
    cr * cr <= hw * hw * dd
}

fn poly_stroke(px: f64, py: f64, sub: &Sub, hw: f64, cap: Cap) -> bool {
    let pts = &sub.pts;
    let n = pts.len();
    let closing = if sub.closed && n > 1 { pts.last().zip(pts.first()) } else { None };
    if pts.windows(2).any(|w| seg_body(px, py, w[0], w[1], hw)) {
        return true;
    }
    if let Some((&a, &b)) = closing {
        if seg_body(px, py, a, b, hw) {
            return true;
        }
    }
    // Joins (rasterised as round) and caps.
    if sub.closed {
        return pts.iter().any(|&v| disk(px, py, v, hw));
    }
    if n > 2 && pts[1..n - 1].iter().any(|&v| disk(px, py, v, hw)) {
        return true;
    }
    if n < 2 {
        return false;
    }
    match cap {
        Cap::Round => disk(px, py, pts[0], hw) || disk(px, py, pts[n - 1], hw),
        Cap::Square => square_cap(px, py, pts[0], pts[1], hw) || square_cap(px, py, pts[n - 1], pts[n - 2], hw),
        Cap::Butt => false,
    }
}

fn edge_cross(a: Pt, b: Pt, px: f64, py: f64) -> i32 {
    let cr = (b.0 - a.0) * (py - a.1) - (px - a.0) * (b.1 - a.1);
    if a.1 <= py && py < b.1 && cr > 0.0 {
        1
    } else if b.1 <= py && py < a.1 && cr < 0.0 {
        -1
    } else {
        0
    }
}

fn filled(px: f64, py: f64, subs: &[Sub], evenodd: bool) -> bool {
    let mut w = 0i64;
    let mut c = 0u64;
    for sub in subs {
        let n = sub.pts.len();
        for (k, &a) in sub.pts.iter().enumerate() {
            let b = sub.pts.get((k + 1) % n).copied().unwrap_or(a);
            let e = edge_cross(a, b, px, py);
            w += i64::from(e);
            c += u64::from(e != 0);
        }
    }
    if evenodd {
        c % 2 == 1
    } else {
        w != 0
    }
}

fn ring(d2: f64, r: f64, hw: f64) -> bool {
    let lo = r - hw;
    let hi = r + hw;
    d2 <= hi * hi && (lo <= 0.0 || d2 >= lo * lo)
}

fn covers(s: &Shape, px: f64, py: f64) -> bool {
    let p = &s.paint;
    let hw = p.width / 2.0;
    match &s.geom {
        Geom::Circle { cx, cy, r } => {
            let d2 = (px - cx) * (px - cx) + (py - cy) * (py - cy);
            (p.fill && d2 <= r * r) || (p.stroke && ring(d2, *r, hw))
        }
        Geom::Rect { cx, cy, hx, hy, rho } => {
            if *hx <= 0.0 || *hy <= 0.0 {
                return false;
            }
            let ax = (px - cx).abs();
            let ay = (py - cy).abs();
            let qx = ax - (hx - rho);
            let qy = ay - (hy - rho);
            let corner = qx > 0.0 && qy > 0.0;
            if p.fill {
                if corner && qx * qx + qy * qy <= rho * rho {
                    return true;
                }
                if !corner && qx.max(qy) <= *rho {
                    return true;
                }
            }
            if p.stroke {
                if *rho == 0.0 && p.join == Join::Miter {
                    // Square corners: the outer box minus the inner box.
                    let outer = ax <= hx + hw && ay <= hy + hw;
                    let inner = ax < hx - hw && ay < hy - hw;
                    return outer && !inner;
                }
                if corner {
                    return ring(qx * qx + qy * qy, *rho, hw);
                }
                return (qx.max(qy) - rho).abs() <= hw;
            }
            false
        }
        Geom::Poly { subs, line } => {
            if p.fill && !line && filled(px, py, subs, p.evenodd) {
                return true;
            }
            p.stroke && subs.iter().any(|sub| poly_stroke(px, py, sub, hw, p.cap))
        }
    }
}

/// Rasterises one 16 px master; `Err` says why it cannot be (`GLYPH-RASTER`).
pub fn rasterise(text: &str) -> Result<Bitmap, String> {
    let mut lint = Vec::new();
    icons_lint::lint_svg_str("glyph", text, Some(16), &mut lint);
    if let Some(f) = lint.first() {
        return Err(format!("outside the icon SVG profile ({} {})", f.code, f.detail));
    }
    let shapes = shapes_of(text)?;
    let mut out = [0u16; SIZE];
    let step = SUPERSAMPLE as f64;
    for (py, row) in out.iter_mut().enumerate() {
        for px in 0..SIZE {
            let mut n = 0u32;
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let x = px as f64 + (sx as f64 + 0.5) / step;
                    let y = py as f64 + (sy as f64 + 0.5) / step;
                    if shapes.iter().any(|s| covers(s, x, y)) {
                        n += 1;
                    }
                }
            }
            if n >= COVER_MIN {
                *row |= 1 << px;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------------------------

/// The closest pair of one group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closest {
    pub a: String,
    pub b: String,
    pub diff: Diff,
}

/// One group's result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupResult {
    pub group: &'static str,
    /// Glyphs rasterised and compared.
    pub glyphs: usize,
    /// The pair with the smallest difference, when at least two glyphs were compared.
    pub min: Option<Closest>,
}

/// Result of `fid-design glyph-distinct`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub threshold: String,
    pub groups: Vec<GroupResult>,
    pub findings: Vec<Finding>,
}

fn read_glyph_index(root: &Path) -> Result<BTreeMap<String, IndexEntry>, String> {
    match review::kind(root, INDEX_FILE)? {
        Kind::Missing => Ok(BTreeMap::new()),
        Kind::File => {
            let text = read_text(&root.join(INDEX_FILE), MAX_INDEX_BYTES)?;
            let entries = parse_index(&text).map_err(|e| format!("{INDEX_FILE}: {e}"))?;
            Ok(entries.into_iter().map(|e| (e.name.clone(), e)).collect())
        }
        _ => Err(format!("{INDEX_FILE} is not a regular file")),
    }
}

fn similar_detail(d: &Diff, t: &Threshold) -> String {
    format!(
        "differ in {} of {} covered pixels ({}) at best shift ({}, {}), below the threshold {}",
        d.symdiff,
        d.union,
        ratio_text(d.ratio_ppm()),
        d.dx,
        d.dy,
        t.text
    )
}

/// Runs `fid-design glyph-distinct` against an icons directory. `Err` is an unreadable root or
/// index (exit 2).
pub fn check(root: &Path, threshold: &Threshold) -> Result<Report, String> {
    let md = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if md.file_type().is_symlink() || !md.is_dir() {
        return Err(format!("--root {} is not a directory", root.display()));
    }
    let index = read_glyph_index(root)?;
    let mut f = Vec::new();
    let mut rasters: BTreeMap<&str, Bitmap> = BTreeMap::new();
    for (_, names) in GROUPS {
        for &name in names {
            let rel = format!("svg/{name}.16.svg");
            let want = format!("icon.{name}");
            let mut why = Vec::new();
            match index.get(name) {
                Some(e) if e.a11y_id.as_deref() == Some(want.as_str()) => {}
                Some(_) => why.push(format!("its index entry needs `a11y_id` = {want:?}")),
                None => why.push(format!("no entry in {INDEX_FILE}")),
            }
            let kind = review::kind(root, &rel)?;
            if kind == Kind::Missing {
                why.push(format!("no 16 px master {rel}"));
            }
            if !why.is_empty() {
                f.push(Finding::new(MISSING, name, why.join("; ")));
            }
            let text = match kind {
                Kind::Missing => continue,
                Kind::File => {
                    match read_text(&root.join("svg").join(format!("{name}.16.svg")), icons_lint::MAX_SVG_BYTES) {
                        Ok(t) => t,
                        Err(e) => {
                            f.push(Finding::new(RASTER, name, e));
                            continue;
                        }
                    }
                }
                _ => {
                    f.push(Finding::new(RASTER, name, format!("{rel} is not a regular file")));
                    continue;
                }
            };
            match rasterise(&text) {
                Ok(b) if count(&b) == 0 => f.push(Finding::new(RASTER, name, "covers no pixel at 16 px")),
                Ok(b) => {
                    rasters.insert(name, b);
                }
                Err(e) => f.push(Finding::new(RASTER, name, e)),
            }
        }
    }

    let mut groups = Vec::new();
    for (group, names) in GROUPS {
        let have: Vec<(&str, &Bitmap)> = names.iter().filter_map(|n| rasters.get(n).map(|b| (*n, b))).collect();
        let mut min: Option<Closest> = None;
        for (i, (a, ba)) in have.iter().enumerate() {
            for (b, bb) in have.iter().skip(i + 1) {
                let Some(d) = best_diff(ba, bb) else { continue };
                if threshold.below(&d) {
                    f.push(Finding::new(SIMILAR, format!("{a} {b}"), similar_detail(&d, threshold)));
                }
                if min.as_ref().map_or(true, |m| d.less(&m.diff)) {
                    min = Some(Closest { a: a.to_string(), b: b.to_string(), diff: d });
                }
            }
        }
        groups.push(GroupResult { group, glyphs: have.len(), min });
    }

    let sil: BTreeMap<&str, Bitmap> = rasters.iter().map(|(n, b)| (*n, silhouette(b))).collect();
    let fam: Vec<&str> =
        CLASSES.iter().copied().filter(|n| FAMILY_MEMBERS.contains(n) && sil.contains_key(n)).collect();
    for (i, a) in fam.iter().enumerate() {
        for b in fam.iter().skip(i + 1) {
            if let (Some(sa), Some(sb)) = (sil.get(a), sil.get(b)) {
                if let Some(d) = best_diff(sa, sb) {
                    if d.symdiff * FAMILY_SAME.1 > FAMILY_SAME.0 * d.union {
                        f.push(Finding::new(
                            FAMILY,
                            format!("{a} {b}"),
                            format!(
                                "outer shapes differ in {} of {} pixels ({}); M1, S1, W3 and A0 must share one",
                                d.symdiff,
                                d.union,
                                ratio_text(d.ratio_ppm())
                            ),
                        ));
                    }
                }
            }
        }
    }
    for o in CLASSES.iter().filter(|n| !FAMILY_MEMBERS.contains(*n)) {
        for m in &fam {
            if let (Some(so), Some(sm)) = (sil.get(o), sil.get(m)) {
                if let Some(d) = best_diff(so, sm) {
                    if threshold.below(&d) {
                        f.push(Finding::new(
                            FAMILY,
                            format!("{o} {m}"),
                            format!(
                                "uses the outer shape of the M1, S1, W3 and A0 family: {}",
                                similar_detail(&d, threshold)
                            ),
                        ));
                    }
                }
            }
        }
    }
    for a in STATES_APART.0 {
        for b in STATES_APART.1 {
            if let (Some(sa), Some(sb)) = (sil.get(a), sil.get(b)) {
                if let Some(d) = best_diff(sa, sb) {
                    if threshold.below(&d) {
                        f.push(Finding::new(
                            FAMILY,
                            format!("{a} {b}"),
                            format!("outer shapes {}", similar_detail(&d, threshold)),
                        ));
                    }
                }
            }
        }
    }

    f.sort();
    f.dedup();
    Ok(Report { threshold: threshold.text.clone(), groups, findings: f })
}

impl Report {
    /// 0 no findings, 1 findings.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let groups = self
            .groups
            .iter()
            .map(|g| {
                let mut m = BTreeMap::new();
                m.insert("group".to_string(), CValue::Str(g.group.to_string()));
                m.insert("glyphs".to_string(), CValue::U64(g.glyphs as u64));
                let min = match &g.min {
                    None => CValue::Null,
                    Some(c) => {
                        let mut x = BTreeMap::new();
                        x.insert("a".to_string(), CValue::Str(c.a.clone()));
                        x.insert("b".to_string(), CValue::Str(c.b.clone()));
                        x.insert("symdiff".to_string(), CValue::U64(c.diff.symdiff));
                        x.insert("union".to_string(), CValue::U64(c.diff.union));
                        x.insert("ratio".to_string(), CValue::Str(ratio_text(c.diff.ratio_ppm())));
                        x.insert(
                            "shift".to_string(),
                            CValue::Arr(vec![CValue::I64(i64::from(c.diff.dx)), CValue::I64(i64::from(c.diff.dy))]),
                        );
                        CValue::Obj(x)
                    }
                };
                m.insert("min".to_string(), min);
                CValue::Obj(m)
            })
            .collect();
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("threshold".to_string(), CValue::Str(self.threshold.clone()));
        m.insert("groups".to_string(), CValue::Arr(groups));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design glyph-distinct: threshold {}\n", self.threshold);
        for g in &self.groups {
            match &g.min {
                Some(c) => s.push_str(&format!(
                    "{}: {} glyph(s), minimum difference {} ({} {})\n",
                    g.group,
                    g.glyphs,
                    ratio_text(c.diff.ratio_ppm()),
                    c.a,
                    c.b
                )),
                None => s.push_str(&format!("{}: {} glyph(s), no pair compared\n", g.group, g.glyphs)),
            }
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    fn svg(body: &str) -> String {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 16 16\">{body}</svg>")
    }

    fn pixels(b: &Bitmap) -> Vec<(usize, usize)> {
        let mut v = Vec::new();
        for (y, r) in b.iter().enumerate() {
            for x in 0..SIZE {
                if (r >> x) & 1 == 1 {
                    v.push((x, y));
                }
            }
        }
        v
    }

    fn raster(body: &str) -> Bitmap {
        match rasterise(&svg(body)) {
            Ok(b) => b,
            Err(e) => panic!("{e}: {body}"),
        }
    }

    /// Hand-labelled: a filled rectangle covers exactly its pixels.
    #[test]
    fn filled_rect() {
        let b = raster("<rect x=\"2\" y=\"3\" width=\"4\" height=\"5\"/>");
        let want: Vec<(usize, usize)> = (3..8).flat_map(|y| (2..6).map(move |x| (x, y))).collect();
        assert_eq!(pixels(&b), want);
    }

    /// Hand-labelled: a 1 px horizontal stroke on the 7.5 centreline from x 2 to 10. Butt and
    /// round caps stay inside columns 2..9 (a round cap covers 6 of 16 samples of the next
    /// pixel); a square cap reaches half of columns 1 and 10 (8 samples each).
    #[test]
    fn line_caps() {
        let base = "<path d=\"M2 7.5H10\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\" stroke-linecap=\"";
        let row = |b: &Bitmap| b[7];
        let butt = raster(&format!("{base}butt\"/>"));
        let round = raster(&format!("{base}round\"/>"));
        let square = raster(&format!("{base}square\"/>"));
        assert_eq!(row(&butt), 0b0000_0011_1111_1100);
        assert_eq!(round, butt);
        assert_eq!(row(&square), 0b0000_0111_1111_1110);
        assert_eq!(count(&butt), 8);
        assert_eq!(count(&square), 10);
    }

    /// Hand-labelled: a filled circle of radius 2 at (8, 8) covers the 4 x 4 block of columns and
    /// rows 6..9 without its corners (a corner pixel holds 6 of 16 samples).
    #[test]
    fn filled_circle() {
        let b = raster("<circle cx=\"8\" cy=\"8\" r=\"2\"/>");
        assert_eq!(
            pixels(&b),
            [(7, 6), (8, 6), (6, 7), (7, 7), (8, 7), (9, 7), (6, 8), (7, 8), (8, 8), (9, 8), (7, 9), (8, 9)]
        );
    }

    /// Hand-labelled: two nested squares drawn the same way round: nonzero fills all 144 pixels,
    /// evenodd leaves the inner 6 x 6 hole.
    #[test]
    fn fill_rules() {
        let d = "M2 2H14V14H2ZM5 5H11V11H5Z";
        assert_eq!(count(&raster(&format!("<path d=\"{d}\"/>"))), 144);
        assert_eq!(count(&raster(&format!("<path d=\"{d}\" fill-rule=\"evenodd\"/>"))), 108);
    }

    /// A sharp stroked square with miter joins has square corners; with round joins the corner
    /// pixel keeps enough samples too (rounding radius 0.5 removes 1 of its 16 samples).
    #[test]
    fn rect_stroke_corners() {
        let miter = raster("<rect x=\"2.5\" y=\"2.5\" width=\"10\" height=\"10\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\"/>");
        let round = raster("<rect x=\"2.5\" y=\"2.5\" width=\"10\" height=\"10\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\" stroke-linejoin=\"round\"/>");
        // The ring of columns and rows 2 and 12: 11 * 11 - 9 * 9 = 40 pixels.
        assert_eq!(count(&miter), 40);
        assert_eq!(miter, round);
        assert_eq!(silhouette(&miter), raster("<rect x=\"2\" y=\"2\" width=\"11\" height=\"11\"/>"));
    }

    /// A straight cubic and quadratic draw the same pixels as the straight line.
    #[test]
    fn straight_curves_match_lines() {
        let style = "fill=\"none\" stroke=\"currentColor\" stroke-width=\"1\" stroke-linecap=\"round\"";
        let line = raster(&format!("<path d=\"M2 7.5H13\" {style}/>"));
        assert_eq!(raster(&format!("<path d=\"M2 7.5C5 7.5 9 7.5 13 7.5\" {style}/>")), line);
        assert_eq!(raster(&format!("<path d=\"M2 7.5Q8 7.5 13 7.5\" {style}/>")), line);
        assert_eq!(raster(&format!("<path d=\"M2 7.5c3 0 7 0 11 0\" {style}/>")), line);
    }

    /// A full circle drawn as two half arcs differs from the circle element by at most one
    /// boundary pixel; an arc rotated 45 degrees with unequal radii is refused.
    #[test]
    fn arcs() {
        let circle = raster("<circle cx=\"8\" cy=\"8\" r=\"5\"/>");
        for d in [
            "M3 8A5 5 0 0 0 13 8A5 5 0 0 0 3 8Z",
            "M3 8A5 5 0 1 1 13 8A5 5 0 1 1 3 8Z",
            "M3 8a5 5 0 0 0 10 0a5 5 0 0 0 -10 0z",
        ] {
            let b = raster(&format!("<path d=\"{d}\"/>"));
            let sd: u32 = b.iter().zip(circle.iter()).map(|(x, y)| (x ^ y).count_ones()).sum();
            assert!(sd <= 1, "{d}: {sd} pixels differ");
        }
        // A quarter arc with sweep 1 bends the other way from sweep 0.
        let s1 = raster("<path d=\"M3 8A5 5 0 0 1 8 3L8 8Z\"/>");
        let s0 = raster("<path d=\"M3 8A5 5 0 0 0 8 3L8 8Z\"/>");
        assert!(count(&s1) > count(&s0), "{} {}", count(&s1), count(&s0));
        assert!(rasterise(&svg("<path d=\"M3 8A5 3 45 0 0 13 8Z\"/>")).is_err());
        assert!(rasterise(&svg("<path d=\"M3 8A5 3 90 0 0 13 8Z\"/>")).is_ok());
    }

    #[test]
    fn profile_and_paint_errors() {
        assert!(rasterise(&svg("<text>x</text>")).is_err());
        assert!(rasterise("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\"><rect x=\"2\" y=\"2\" width=\"4\" height=\"4\"/></svg>").is_err());
        assert!(rasterise(&svg("<rect x=\"2\" y=\"2\" width=\"4\" height=\"4\" opacity=\"0.5\"/>")).is_err());
        assert!(rasterise(&svg("<rect x=\"2\" y=\"2\" width=\"4\" height=\"4\" rx=\"1\" ry=\"0.5\"/>")).is_err());
        assert!(rasterise(&svg("<rect x=\"2\" y=\"2\" width=\"4\" height=\"4\" opacity=\"1\"/>")).is_ok());
        assert_eq!(count(&raster("<g fill=\"none\"><rect x=\"2\" y=\"2\" width=\"4\" height=\"4\"/></g>")), 0);
    }

    #[test]
    fn shifts_and_metric() {
        let a = raster("<rect x=\"3\" y=\"3\" width=\"5\" height=\"5\"/>");
        let b = raster("<rect x=\"4\" y=\"2\" width=\"5\" height=\"5\"/>");
        assert_eq!(shifted(&b, -1, 1), a);
        assert_eq!(best_diff(&a, &b), Some(Diff { symdiff: 0, union: 25, dx: -1, dy: 1 }));
        let c = raster("<rect x=\"3\" y=\"3\" width=\"5\" height=\"6\"/>");
        // 5 of 30 at no shift and at one row up; the first in row-then-column order is kept.
        assert_eq!(best_diff(&a, &c), Some(Diff { symdiff: 5, union: 30, dx: 0, dy: -1 }));
        assert_eq!(shifted(&[0x8001; SIZE], 1, 0), [0x0002; SIZE]);
        assert_eq!(shifted(&[0x8001; SIZE], -1, 0), [0x4000; SIZE]);
        assert_eq!(best_diff(&[0; SIZE], &[0; SIZE]), None);
    }

    #[test]
    fn thresholds() {
        let t = Threshold::parse("0.20").ok();
        assert_eq!(t, Some(Threshold { text: "0.20".into(), num: 20, den: 100 }));
        assert_eq!(Threshold::parse(".25").map(|t| (t.num, t.den)).ok(), Some((25, 100)));
        assert_eq!(Threshold::parse("1").map(|t| (t.num, t.den)).ok(), Some((1, 1)));
        for bad in ["0", "0.0", "1.5", "2", "", ".", "1.", "-0.2", "0.1234567", "abc", "0,2", "10"] {
            assert!(Threshold::parse(bad).is_err(), "{bad}");
        }
        let t = Threshold::default_value();
        assert!(t.below(&Diff { symdiff: 19, union: 100, dx: 0, dy: 0 }));
        assert!(!t.below(&Diff { symdiff: 20, union: 100, dx: 0, dy: 0 }));
        assert_eq!(ratio_text(Diff { symdiff: 25, union: 88, dx: 0, dy: 0 }.ratio_ppm()), "0.284090");
    }
}
