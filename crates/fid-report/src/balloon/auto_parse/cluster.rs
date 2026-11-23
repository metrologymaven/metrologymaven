//! Pure-integer deterministic spatial union-find clustering of `TextRun`s into multi-line
//! callouts, stacked bilateral tolerances (`AC-2`) and compartmentalised GD&T frames.

use crate::balloon::auto_text::{SheetRect, TextRun};
use std::collections::BTreeMap;

/// A spatially clustered group of one or more [`TextRun`]s ready for callout grammar parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCluster {
    pub runs: Vec<TextRun>,
    pub bbox: SheetRect,
    pub stacked_tolerance: bool,
    pub in_frame_box: bool,
}

/// Deterministically clusters `runs` using integer `0.01 mm` font-height-scaled proximity rules
/// and optional callout `frame_boxes` (`Design: Algorithm`, `AC-2`).
pub fn cluster_runs(runs: &[TextRun], frame_boxes: &[SheetRect]) -> Vec<RunCluster> {
    let n = runs.len();
    if n == 0 {
        return Vec::new();
    }

    let mut dsu = Dsu::new(n);
    let mut stacked_pair = vec![false; n];
    let mut framed = vec![false; n];

    // 1. Merge runs sharing the same callout frame box (GD&T FCF compartments).
    for fb in frame_boxes {
        let mut first: Option<usize> = None;
        for (i, r) in runs.iter().enumerate() {
            if rect_center_inside(r.bbox, *fb) {
                framed[i] = true;
                if let Some(f) = first {
                    dsu.union(f, i);
                } else {
                    first = Some(i);
                }
            }
        }
    }

    // 2. Pairwise proximity merge:
    //    - Stacked bilateral tolerances: a run starting with `+` or `-` (signed tolerance)
    //      positioned directly to the right of a numeric nominal run (or stacked directly above/below
    //      another signed tolerance run at the same x column).
    //    - Same-line continuation fragments within 0.8 * font_h horizontally with strong vertical overlap.
    for i in 0..n {
        let ri = &runs[i];
        let ti = ri.text.untrusted_value().as_str().trim();
        let fh_i = ri.font_h_cmm.max(ri.bbox.height()).max(100);

        for j in (i + 1)..n {
            let rj = &runs[j];
            if (ri.angle_mdeg - rj.angle_mdeg).abs() > 5_000 {
                continue;
            }
            let tj = rj.text.untrusted_value().as_str().trim();
            let fh = fh_i.max(rj.font_h_cmm.max(rj.bbox.height()).max(100));

            // Quick vertical reject before finer checks.
            let v_gap = axis_gap(ri.bbox.y0, ri.bbox.y1, rj.bbox.y0, rj.bbox.y1);
            if v_gap > fh * 3 {
                continue;
            }
            let h_gap = axis_gap(ri.bbox.x0, ri.bbox.x1, rj.bbox.x0, rj.bbox.x1);
            if h_gap > fh * 4 {
                continue;
            }

            // Case A: Two stacked signed tolerances (`+0.018` over `-0.000`).
            if is_signed_tol_fragment(ti) && is_signed_tol_fragment(tj) {
                let x_align = (ri.bbox.x0 - rj.bbox.x0).abs() <= fh * 2
                    || axis_overlap(ri.bbox.x0, ri.bbox.x1, rj.bbox.x0, rj.bbox.x1) > 0;
                if x_align && v_gap <= fh * 2 {
                    dsu.union(i, j);
                    stacked_pair[i] = true;
                    stacked_pair[j] = true;
                    continue;
                }
            }

            // Case B: Nominal run paired with an adjacent signed tolerance fragment to its right.
            let (nom_r, tol_r) = if is_signed_tol_fragment(tj) && !is_signed_tol_fragment(ti) {
                (Some((i, ri)), Some((j, rj)))
            } else if is_signed_tol_fragment(ti) && !is_signed_tol_fragment(tj) {
                (Some((j, rj)), Some((i, ri)))
            } else {
                (None, None)
            };
            if let (Some((_, nr)), Some((_, tr))) = (nom_r, tol_r) {
                let dx = tr.bbox.x0 - nr.bbox.x1;
                let ny_mid = (nr.bbox.y0 + nr.bbox.y1) / 2;
                let ty_mid = (tr.bbox.y0 + tr.bbox.y1) / 2;
                if dx >= -fh / 2 && dx <= fh * 3 && (ty_mid - ny_mid).abs() <= (fh * 22) / 10 {
                    dsu.union(i, j);
                    stacked_pair[i] = true;
                    stacked_pair[j] = true;
                    continue;
                }
            }

            // Case C: Tight same-line fragments (e.g. `"4X"` next to `"Ø6.50"` or FCF pieces).
            let v_ov = axis_overlap(ri.bbox.y0, ri.bbox.y1, rj.bbox.y0, rj.bbox.y1);
            let min_h = ri.bbox.height().min(rj.bbox.height()).max(1);
            if v_ov * 2 >= min_h && h_gap <= (fh * 8) / 10 {
                dsu.union(i, j);
            }
        }
    }

    // Group runs by canonical DSU root in deterministic BTreeMap order.
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..n {
        let root = dsu.find(i);
        groups.entry(root).or_default().push(i);
    }

    let mut out = Vec::with_capacity(groups.len());
    for members in groups.into_values() {
        let is_stacked = members.iter().any(|&idx| stacked_pair[idx]);
        let is_framed = members.iter().any(|&idx| framed[idx]);
        let mut group_runs: Vec<TextRun> = members.into_iter().map(|idx| runs[idx].clone()).collect();
        // Order runs inside a cluster: left-to-right primarily, then top-to-bottom for stacked tolerances!
        group_runs.sort_by(|a, b| {
            let same_col = (a.bbox.x0 - b.bbox.x0).abs() <= a.font_h_cmm.max(b.font_h_cmm).max(100);
            if same_col {
                a.bbox.y0.cmp(&b.bbox.y0).then_with(|| a.bbox.x0.cmp(&b.bbox.x0)).then_with(|| a.id.cmp(&b.id))
            } else {
                a.bbox.x0.cmp(&b.bbox.x0).then_with(|| a.bbox.y0.cmp(&b.bbox.y0)).then_with(|| a.id.cmp(&b.id))
            }
        });

        let mut min_x = i64::MAX;
        let mut min_y = i64::MAX;
        let mut max_x = i64::MIN;
        let mut max_y = i64::MIN;
        for r in &group_runs {
            min_x = min_x.min(r.bbox.x0);
            min_y = min_y.min(r.bbox.y0);
            max_x = max_x.max(r.bbox.x1);
            max_y = max_y.max(r.bbox.y1);
        }

        out.push(RunCluster {
            runs: group_runs,
            bbox: SheetRect::new(min_x, min_y, max_x, max_y),
            stacked_tolerance: is_stacked,
            in_frame_box: is_framed,
        });
    }

    // Deterministic reading order: top-to-bottom, then left-to-right.
    out.sort_by(|a, b| a.bbox.y0.cmp(&b.bbox.y0).then_with(|| a.bbox.x0.cmp(&b.bbox.x0)));
    out
}

fn is_signed_tol_fragment(s: &str) -> bool {
    let trimmed = s.trim();
    if let Some(rest) = trimmed.strip_prefix('+').or_else(|| trimmed.strip_prefix('-')) {
        let rest = rest.trim_end_matches('°');
        !rest.is_empty()
            && rest.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            && rest.bytes().any(|b| b.is_ascii_digit())
    } else {
        false
    }
}

fn rect_center_inside(inner: SheetRect, outer: SheetRect) -> bool {
    let cx = (inner.x0 + inner.x1) / 2;
    let cy = (inner.y0 + inner.y1) / 2;
    cx >= outer.x0 && cx <= outer.x1 && cy >= outer.y0 && cy <= outer.y1
}

fn axis_gap(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    if a1 < b0 {
        b0 - a1
    } else if b1 < a0 {
        a0 - b1
    } else {
        0
    }
}

fn axis_overlap(a0: i64, a1: i64, b0: i64, b1: i64) -> i64 {
    (a1.min(b1) - a0.max(b0)).max(0)
}

struct Dsu {
    parent: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu { parent: (0..n).collect() }
    }

    fn find(&mut self, mut x: usize) -> usize {
        while self.parent[x] != x {
            let p = self.parent[x];
            self.parent[x] = self.parent[p];
            x = p;
        }
        x
    }

    fn union(&mut self, a: usize, b: usize) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            if ra < rb {
                self.parent[rb] = ra;
            } else {
                self.parent[ra] = rb;
            }
        }
    }
}
