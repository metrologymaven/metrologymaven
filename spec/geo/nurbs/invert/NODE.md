---
id: GEO.NURBS.INVERT
title: Point inversion on NURBS curves and surfaces
level: 3
parent: GEO.NURBS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-nurbs]
built_pr: 271
satisfies: [REQ-GEO.NURBS-005, REQ-GEO.NURBS-006]
covers: []
depends_on: [GEO.NURBS.EVAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GEO.NURBS.INVERT: Point inversion on NURBS curves and surfaces

## Purpose

Finds the globally closest point on a single untrimmed NURBS curve or surface to a query point, returning parameters, foot point and distance. GEO.DIST builds trimmed-face and body projection on top of it, so it must be robust (global, not just local) and deterministic.

## Scope

**In:**
- Candidate generation by Bézier-patch decomposition (knot insertion) and bounding-box distance pruning
- Newton iteration on each surviving candidate with domain clamping and periodic wrap
- Boundary checks: closest point on the four boundary curves (or two curve ends)
- Convergence criteria, iteration limits and the typed non-convergence error
- Tie-breaking between equally close candidates

**Out:**
- Analytic closed-form inversion (owned by `GEO.NURBS.ANALYTIC`)
- Trimming and multi-face search (owned by `GEO.DIST.FACE`)

## Requirements

- **REQ-GEO.NURBS.INVERT-001** When a point is inverted onto a surface, the system shall return the parameter pair, foot point and distance of the global closest point on the untrimmed surface, including its boundary.
- **REQ-GEO.NURBS.INVERT-002** The result shall satisfy either the orthogonality condition |S_u·(S−P)| ≤ 1e-12·‖S_u‖·max(‖S−P‖, 1e-9 m) (and likewise for v), or lie on the domain boundary.
- **REQ-GEO.NURBS.INVERT-003** If Newton iteration does not meet the criterion within 50 iterations for the best candidate, then the system shall return `NonConvergence { best }` with the best point found.
- **REQ-GEO.NURBS.INVERT-004** When two candidates are within 1e-15 m of the same distance, the system shall choose the one with the lexicographically smallest (u, v).
- **REQ-GEO.NURBS.INVERT-005** Candidate pruning shall be conservative: a patch is discarded only if its control-polygon bounding box is provably farther than the current best distance.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/nurbs/ref_set_v1` inversion queries (5 000 points, including points near several local minima), when inverted, then every foot point is within 1e-12 m of the reference global closest point and distances agree within 1e-12 m.
- **AC-2** Given fixture `fixtures/geo/nurbs/multi_minima` (a wavy surface with query points equidistant to two crests), when inverted, then the lexicographically smallest (u, v) is chosen.
- **AC-3** Given a query point whose closest point lies on a boundary edge, when inverted, then the boundary parameter is returned exactly at the domain limit.
- **AC-4** (error path) Given a synthetic surface with a cusp that defeats Newton (in the fixture), when inverted, then `NonConvergence` is returned with a finite best point.
- **AC-5** Given `ref_set_v1`, when run on three targets, then the result digest is identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `Inversion { u, v, foot, distance, on_boundary, uniqueness }` and `CurveInversion { t, foot, distance, on_boundary, uniqueness }` (`uniqueness` is the `Uniqueness { Unique, NonUnique }` of GEO.NURBS.ANALYTIC); `InvertError<I> { NonConvergence { best: I }, NonFiniteQuery, NonFinite, Eval(EvalError) }`. `on_boundary` is true when a returned parameter equals a limit of its domain. `foot` is the GEO.NURBS.EVAL position at the returned parameters and `distance = sqrt((r0 r0 + r1 r1) + r2 r2)`, `r = foot - q`.
- **Algorithm:** decompose into Bézier patches by knot insertion; order patches by lower-bound distance (box distance), then by patch index; for each patch whose lower bound is below the current best, seed Newton at the patch's closest control-point parameter and iterate with clamped steps; finally test boundary curves via curve inversion. After Piegl and Tiller, *The NURBS Book*, §6.1 (cited, not copied) plus subdivision pruning. The pinned form of this, with the exclusion and uniqueness tests that make the search provably global, is below.
- **Pinned algorithm (build PR #271; every value-affecting choice; mirrored bit for bit by `fixtures/geo/nurbs/invert/invert_model.py`):**
  1. *Scale.* `M` = largest absolute coordinate of the control points and the query; `L2` = squared diagonal and `X` = largest extent of the control points' axis-aligned box. Homogeneous points `(w x, w y, w z, w)` (`w = 1` without weights).
  2. *Bézier decomposition.* Per direction, every distinct knot value of the domain `[U[p], U[n]]`, ascending, is inserted (Boehm, one at a time, `Q_i = alpha P_i + (1 - alpha) P_(i-1)`, `alpha = (x - U[i]) / (U[i+p] - U[i])`, `i = k-p+1 ..= k-m`) until its multiplicity is at least `p`; surfaces insert along `u` on every column, then along `v` on every row. Piece `(iu, iv)` covers `[x_iu, x_iu+1] x [y_iv, y_iv+1]` with control points from index `s - p` (`s` the last index of `x_iu`). Unclamped knot vectors need no special case.
  3. *Bernstein tests.* For a node (a Bézier piece or a dyadic part of one, local parameters `s`, `t` in `[0, 1]`) and the query `q`: `G_s = sum_c (dA_c (x) w - A_c (x) dw) (x) (A_c - q_c w)` over `c = x, y, z` in that order (`d` = forward difference along `s`, `(x)` the Bernstein product with weights `(C(m,i) C(n,j)) / C(m+n,i+j)` from a binary64 Pascal table, sums with `i` then `j` ascending from `0.0`), so `G_s = w^3 S_s . (S - q)` up to a positive factor; `Gm` the same with absolute values and `|A_c| + |q_c| w`. Margin `GAMMA Gm`, `GAMMA = 2^-36`. **Excluded** (no stationary point in the closed node): all coefficients of `G_s` (or of `G_t`) `> GAMMA Gm` or all `< -GAMMA Gm`; for a one-parameter node also when the differences of `G` are all `< -GAMMA (Gm_k+1 + Gm_k)` (one stationary point, a maximum). **Unique** (at most one stationary point, a minimum): one parameter, all differences `> GAMMA (Gm_k+1 + Gm_k)`; two parameters, `a > 0`, `d > 0`, `a d > b^2` with `a = m1 min(dG_s/ds coefficient bound)`, `d = n2 min(dG_t/dt bound)`, `b = (b1 + b2) / 2`, `b1 = n1 max |dG_s/dt|`, `b2 = m2 max |dG_t/ds|` (each difference less, or plus, its margin): the symmetric part of the Jacobian of `(G_s, G_t)` is positive definite on the node, so `(G_s, G_t)` is injective and its zero, if any, is a minimum. `G` is recomputed from each node's own control net.
  4. *Lines and breakpoints.* Curve: every distinct domain knot value is a candidate. Surface: the lines `u = ` start, each interior knot of multiplicity `p` (a possible crease), end (ascending), then the same in `v`; each line's Bézier pieces are rows or columns of the patch nets; every distinct knot value of the other direction on the line is a candidate, then the line is searched by step 5 with one parameter. A line (or a whole curve) whose projected control points all agree within `DEGEN_REL X` (`2^-40`) is a pole: one candidate at its start, flagged.
  5. *Branch and bound.* Nodes in a binary heap keyed by `(lower bound bits, sequence number)`; root pieces first in order. Lower bound = distance from `q` to the box of the projected control points; the closest projected control point (first in row-major order) is the seed, at `lerp(s0, s1, i / p)`. Pop: stop when `lb - 2^-48 M > best + TIE` (REQ-005); skip excluded nodes; for unique nodes run Newton (step 6) in the node box from the seed; otherwise halve the node (along `t` when its largest control-point difference along `t` exceeds that along `s`, else along `s`; de Casteljau at 1/2, midpoints `(a + b) * 0.5`; children in low, high order) while `depth < MAX_DEPTH = 60` and fewer than `BUDGET = 1024` halvings were made in this inversion; else the node is unresolved and its seed point is a candidate. Every candidate's foot and distance come from the GEO.NURBS.EVAL position at `lerp(x0, x1, s)` (`lerp(a, b, f) = a` at `f = 0`, `b` at `f = 1`, else `a + f (b - a)` clamped to `[a, b]`).
  6. *Newton.* On the root piece's Bézier net (de Casteljau in `s`, `t`: rows along `t` first, then along `s`; derivatives from the last levels, quotient rule as GEO.NURBS.EVAL), in the node box. Per iteration: `r = S - q`, `g = (S_s . r, S_t . r)`, fundamental forms `E, F, G`, Hessian `H = I + (r . S_ss, r . S_st, r . S_tt)`. Box test per direction: at the lower bound with `g >= 0`, at the upper bound with `g <= 0`, or `g^2 <= e T^2` with `T = max(1e-12 max(d, 1e-9), NOISE M)`, `NOISE = 2^-46`. After the test first holds, one more (polishing) step is taken. Step: the Newton step with `H` if `h_uu > 0` and `det H > 0`, else with `I` if `E > 0` and `det I > 0`, else `-g / (E + G)`; a direction at a bound with the gradient pointing out is fixed. Step halving from `lambda = 1` at most `MAX_HALVINGS = 30` times, clamped to the box, accepted when `f <= f0 + (NOISE M)(2 d + NOISE M)` (rounding noise); stop when the trial point equals the current one. At most `MAX_ITER = 50` iterations (REQ-003).
  7. *Selection (REQ-004).* `dmin` = smallest candidate distance; among candidates with `d <= dmin + TIE` (`TIE = 1e-15` m), the lexicographically smallest `(u, v)` (first in generation order on equality); a pole candidate tied with it and within `FOOT_SEP = 1e-9` m (infinity norm) of its foot replaces it (canonical pole parameters).
  8. *Uniqueness.* `NonUnique` when a tied candidate's foot is more than `FOOT_SEP` from the winner's, when an unresolved node with `lb <= dmin + TIE` has a control point more than `FOOT_SEP` from the winner's foot (curvature centres, budget exhausted), or when the winner is a pole.
  9. *Certification (REQ-002, REQ-003).* With EVAL derivatives at the winner, per direction: skipped for a crease or kink value (interior knot of multiplicity `p`) or along a pole; else at the domain lower limit with `g >= 0` or upper limit with `g <= 0`, else not certified when `|S_x|^2 W_x^2 <= 2^-52 L2` in the interior (a cusp or other singular point), else `g^2 <= e T^2`. Not certified: `NonConvergence { best }`.
- **Deviations from the original Design (stated in the PR):** subdivision with Bernstein exclusion and uniqueness tests replaces "Newton per surviving patch" (which can miss a second minimum inside one patch); Newton runs on the Bézier piece in the node box rather than on the whole domain, so periodic wrap is unnecessary (periodic seams are boundary lines, and ties at them resolve to the smaller parameter); REQ-002's tolerance has the evaluation-noise floor `NOISE M` (the literal `1e-12 x 1e-9 m` is below binary64 resolution for on-surface queries); interior C0 knot lines are searched like boundary curves and count as boundaries for certification; a point where a partial derivative vanishes in the interior (cusp) is not certifiable, which is the AC-4 behaviour.
- **Errors:** `NonConvergence`, `NonFiniteQuery`, `NonFinite`, `Eval`.
- **Determinism:** D0 (fixed patch order, fixed iteration count limits, KNP-1 arithmetic; no FMA, no `f64::min`/`max`, no hash containers; heap keyed by bit patterns and sequence numbers).
- **Performance budget:** median 5 µs per inversion on a bicubic 10 × 10 patch surface [unverified]; the pinned Bernstein tests cost more (about 0.1 to 1 ms per surface inversion in an optimised build [unverified]).
- **Crate:** `fid-geo-nurbs` (kernel graph, KNP-1 applies).

## Test fixtures

- `ref_set_v1` and `multi_minima` from GEO.NURBS.REF (references from dense high-precision search). CC-BY-4.0. Known defect: `sphere-q0121` is a local, not the global, minimum (the radial projection is 2.7e-6 m closer); tests check the sphere against its closed form.
- `fixtures/geo/nurbs/invert/fuzz_set.json` (seed 44): curvature centres and axes, sphere poles, kinks and creases, unclamped and periodic knot vectors, degrees up to 25, near ties and random rational curves and surfaces, referenced by the GEO.NURBS.REF oracle and closed forms; `digests-v1.txt` from the bit-exact model `invert_model.py`; both written by `make_invert_fixtures.py`. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-nurbs` (pure Rust, no I/O, KNP-1), add module `invert` with `invert_surface(s: &NurbsSurface, p: [f64;3]) -> Result<Inversion, InvertError>` and `invert_curve`. Algorithm: Bézier decomposition by knot insertion, conservative box-distance pruning, patches processed in (lower bound, patch index) order, Newton per surviving patch with clamping and periodic wrap, then boundary-curve checks. Converge when the orthogonality residual ≤ 1e-12 relative (REQ-002) or the point is on the boundary; at most 50 iterations, else `NonConvergence{best}`. Ties within 1e-15 m: smallest (u, v). Acceptance: AC-1 foot points within 1e-12 m of references for 5 000 queries; AC-2 tie rule; AC-3 boundary exactness; AC-4 non-convergence case; AC-5 identical digest on three targets. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1, no GPU. Files: `crates/fid-geo-nurbs/src/invert.rs`, `src/bezier.rs`, `tests/invert.rs`.

## Open questions

- None.

## Children

_Leaf: none._
