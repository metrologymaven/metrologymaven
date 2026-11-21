---
id: KRN.ENV.MCC.CYL
title: Minimum circumscribed cylinder
level: 4
parent: KRN.ENV.MCC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV.MCC-001, REQ-KRN.ENV.MCC-003, REQ-KRN.ENV.MCC-005]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.TRAIT, KRN.LSQ.AXIS.CYL, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART, KRN.MAN.CARD]
uses_artifacts: [UNC.RDL.FREEZE]
side_effect: W2
determinism: D0
cli: [fid fit mcc-cylinder]
mcp: [fit_mcc_cylinder]
journal: [fit.mcc_cylinder@1]
updated: 2026-11-20
---

# KRN.ENV.MCC.CYL: Minimum circumscribed cylinder

## Purpose

The smallest cylinder containing all points: the mating envelope of a shaft. Unlike the circle and sphere, the problem is non-convex in the axis direction, so it is solved by sequential linearisation about the least-squares axis with a deterministic multi-start, and the result says plainly when global optimality is not proven.

## Scope

**In:** algorithm `fit.cylinder.mcc@1.0.0`; axis parameterised by two tangent-plane parameters about the current axis; each linearised step an SOCP through `KRN.SOLV.SEQ`; multi-start from `KRN.LSQ.AXIS.CYL` through `KRN.SOLV.MSTART`; contact set, local optimality check and `local_optimum_only`; optional fixed axis direction (for an envelope constrained to a datum direction); the command; a spec card.
**Out:** circle and sphere (`KRN.ENV.MCC.CVX`); least-squares cylinder (`KRN.LSQ.AXIS.CYL`); actual mating envelope rules under a dialect (`GDT`).

## Requirements

- **REQ-KRN.ENV.MCC.CYL-001** The module shall compute the minimum circumscribed cylinder implementing the `Fit` trait, starting from the least-squares cylinder axis and running every start of the deterministic multi-start.
- **REQ-KRN.ENV.MCC.CYL-002** The result shall carry the contact set, the radius recomputed from the contact set and final axis, the multi-start agreement flag, the local optimality check outcome, and `local_optimum_only: true` whenever starts disagree beyond tolerance or the local check fails.
- **REQ-KRN.ENV.MCC.CYL-003** When a fixed axis direction is given, the module shall solve the convex 2D problem in the plane perpendicular to that direction and shall report `local_optimum_only: false`.
- **REQ-KRN.ENV.MCC.CYL-004** If the points' axial extent is below the conditioning threshold relative to the radius (a disc rather than a cylinder), then the fit shall return `IllConditioned` with the ratio.
- **REQ-KRN.ENV.MCC.CYL-005** When the command is committed, the system shall journal one `fit.mcc_cylinder@1` event with algorithm ID, parameter digest, input digest, seed and output digest; a dry run shall return the R1 result and journal nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/fit/mcc-cyl-v1` (seeded cylinders with known form error, full and half circumference, 1 000 to 200 000 points), when `fid fit mcc-cylinder --seed 7 --json` runs, then the radius is within 1e-8 m of the generator's analytic envelope [unverified] and every point lies inside within 1e-12 m.
- **AC-2** Given a case built with two near-equal envelope axes, then the result reports `agreement: false` and `local_optimum_only: true`.
- **AC-3** Given a fixed axis direction, then the result equals `fit.circle2d.mcc` on the projected points within 1e-12 m and `local_optimum_only` is false.
- **AC-4** (error path) Given a 0.1 mm-thick disc of points of radius 20 mm, then the fit returns `IllConditioned` and journals nothing.
- **AC-5** Given AC-1 on x86-64, ARM64 and wasm32 with the same seed, then output digests are identical; replay in exact mode is `IDENTICAL`.

## Design

- **Registry command(s):** `fit.mcc_cylinder@1`, W2 (dry-run R1). Params: point-set handle (`PointsView`), seed, optional fixed axis direction, `SeqConfig`, `StartSpec` overrides. Result: `FittedFeature::Cylinder`, contact set, certificate, multi-start summary, residual statistics.
- **Algorithm:** at each iterate, linearise distances to the axis in the two tangent-plane parameters plus centre offsets and radius; solve the SOCP step; accept by the `KRN.SOLV.SEQ` merit rule; certify at the end; multi-start through `fid_solv::mstart` with the `KRN.LSQ.INIT`-style start spec from the least-squares fit.
- **Errors:** `TooFewPoints`, `IllConditioned`, `NotConverged`, `Solver`, `MultiStart(AllFailed)`. As built: no certified start is `SeqFailed` (code `solver`) with the first start's cause; also `SideViolation`, `ImplausibleFit` and, through the trait only, `FlaggedFit`.
- **Guards (as built).** Multi-start agreement is not proof of correctness (reviews of #358 and #362). (1) Certificate: the least-squares axis with radius `R0`, the largest point distance to it, is a feasible cylinder, so the global minimum is at most `R0`; a certified radius above `(1 + 1e-9) R0` is `ImplausibleFit` (not the global optimum). (2) Flag `suspect_axis` (axis weakly determined or wrong): the zone about the fitted axis (radius minus the smallest point distance) above 3 times the zone about the least-squares axis [unverified threshold]; on the planted envelopes the ratio is 0.93 to 0.99. (3) A point outside the recomputed radius by more than 1e-12 m is `SideViolation`. (4) The `Fit` trait refuses a fit with `local_optimum_only` or `suspect_axis`, and a strict local minimum whose global cross-check did not pass (`FlaggedFit`, fail closed); `fit_mcc_cylinder` returns flagged fits with their flags. (5) Global cross-check of a free-direction fit (reviews of #374): every start lies in a small box about the least-squares axis, so a certified strict local minimum can be far from the global one, and checks along a few fixed directions miss tilted better axes. A branch and bound covers every axis direction with caps (the icosahedron's faces, one of each antipodal pair, split into four; best first). Over a cap with centre `C` and chord radius `h`, the smallest enclosing cylinder of any direction in it is at least `(|w x C| - |w| h) / 2` for a point pair `w`, and at least `mec(S, C) - sqrt(R^2 + max_S |q|^2) h` for a point set `S` (proof in `mcc_cyl/global.rs`); a feasible cylinder at `C` (smallest enclosing circle of the projections, checked over every point) bounds it above. With `tol = 1e-9 R` plus the certificate's value bound: a feasible cylinder below `R - tol` is `better`; caps whose lower bound is at least `R - tol` are decided; others are split, and a cap undecided at depth 40, or more than 2^18 cap tests or 2^12 full tests, is `inconclusive`. Anything but `passed` makes the fit `local_optimum_only` (refused by the trait). The smaller cylinder is not returned: it is feasible, not a certified minimum.
- **Determinism:** D0 for a given seed (the seed is journaled).
- **Performance budget:** 50 000 points with 8 starts < 500 ms [unverified until measured].
- **Crate:** `fid-fit` (module `mcc_cyl`).

## Test fixtures

`fixtures/fit/mcc-cyl-v1`: seeded synthetic generator with analytic envelopes, CC-BY-4.0; open reference cases from the RDL release (`UNC.RDL.FREEZE`) at CI time.

## Build prompt

> In crate `fid-fit` (pure Rust, f64, no I/O, no `unsafe`; depends on `fid-num` and `fid-solv`), add module `mcc_cyl` implementing the `Fit` trait for `fit.cylinder.mcc@1.0.0`: start from the least-squares cylinder, parameterise the axis by two tangent-plane parameters, solve linearised SOCP steps via `fid_solv::seq::solve`, run all starts via `fid_solv::mstart`, certify via `fid_solv::cert::certify`, recompute the radius from the contact set, set `local_optimum_only` when starts disagree or the local check fails; with a fixed axis direction solve the convex 2D problem instead; return `IllConditioned` for disc-like data. Add the registry data row `fit.mcc_cylinder@1` (W2, dry-run R1, D0 per seed; CLI `fid fit mcc-cylinder`; MCP `fit_mcc_cylinder`; one journal event with algorithm ID, parameter digest, input digest, seed, output digest) and a spec card. Tests: `fixtures/fit/mcc-cyl-v1` analytic envelopes, ambiguous-axis case, fixed-direction equivalence to the 2D circle, disc error, cross-target digests on x86-64/ARM64/wasm32, exact replay IDENTICAL. KNP-1: no FMA, transcendentals via `fid_num::f`, no `f64::min`/`max`, no HashMap/HashSet, no SIMD dispatch; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.ENV.MCC.CYL.

## Open questions

- No NMI reference exists for the MCC cylinder [unverified]; evidence is synthetic and open-suite only.
- **Partial build (`fid-fit` module `mcc_cyl`, not marked built).** Done: the certified fit (`fid_solv::seq` steps, `fid_solv::cert` certificates, `fid_solv::mstart` starts from the least-squares cylinder of `KRN.LSQ.AXIS.CYL` anchored at mid-height), the fixed-direction convex fit, the disc test, the guards above, the spec card `crates/fid-fit/cards/fit.cylinder.mc@1.0.0.toml`, the command data row, and in `crates/fid-fit/tests/mcc_cyl.rs` AC-1 on the fixture, AC-3 (except the equality with `fit.circle2d.mcc`), AC-4 and AC-5's per-target output pin. Remaining: (1) the registry, CLI, MCP and journal layer (REQ-005, AC-4's "journals nothing", AC-5's exact replay), which waits for `CORE.REG` to read kernel fit rows, as for `KRN.LSQ.LIN` and `KRN.LSQ.AXIS.CYL`; (2) AC-3's equality with `fit.circle2d.mcc`, which waits for `KRN.ENV.MCC.CVX`; (3) AC-2 (below); (4) the RDL open reference cases (`UNC.RDL.FREEZE`).
- **AC-2 not met as built.** The case built (`twoaxes`: a slab 1.00 x 1.02 x 0.25 whose enclosing cylinders along its two long sides are near-equal) has its envelope axes about 90 degrees from the least-squares frame. The tangent-plane parameters are singular there (the same reach limit as `KRN.LSQ.AXIS.CYL`): with 600 points no start certified (rank-deficient active-set system); with the fixture's 200 points a start certifies a local minimum larger than the least-squares-axis radius, which the certificate guard refuses (`implausible_fit`). Either way the fit fails closed rather than reporting `agreement: false`. A case with two near-equal envelope axes within the start box of a shaft-like point set, and a parameterisation that reaches far axes (or a per-step re-anchoring hook in `KRN.SOLV.NLLS` / `KRN.SOLV.SEQ`), are open.
- **AC-1 point counts.** The fixture has 1 000 and 2 000 points, not up to 200 000: each `seq` step is a dense LP over every point and the debug-build test of all cases already takes minutes. The performance budget is unmeasured [unverified].
- **Choices made at build time (confirm in review).** (a) Algorithm ID, command and names use the contract's method slug `mc` (`fit.cylinder.mc@1.0.0`, `fit.mc_cylinder@1`), as `KRN.LSQ.LIN` did; the leaf says `mcc`. (b) Starts: 8 seeded starts in the box `[w0 / L, w0 / L, w0, w0]` (the tilt and offset the least-squares zone `w0` allows over the axial length `L`), each lifted to a feasible radius. (c) The radius's step scale is 2^20 times the others', so an infeasible iterate can always be repaired (without it short arcs ended `subproblem_infeasible`). (d) The nonlinear certificate's Gauss-Newton start (`start_from_set`) is a Levenberg-Marquardt solve of the set's five equations from the least-squares frame: data and set only, never an iterate. (e) A flagged least-squares start (`local_optimum_only` or `suspect_axis`) makes the envelope `local_optimum_only`. (f) The disc test uses the smallest principal axis: axial thickness below 0.01 of the half width is `IllConditioned` with `cond = width / thickness` [unverified threshold].
- **Tied contacts.** `KRN.SOLV.CERT.AS` breaks ties over the cap only for linear problems (#352); this problem is nonlinear, so with more than five points on the envelope only the first candidate sets are tried. On the fixture's `tie` case (a sixth point on the planted surface at a zero-multiplier position) the fit certifies a five-point subset with the reference radius; when no candidate certifies the fit fails closed (`solver`).
- **Degenerate active sets (oval cross-sections).** The certifier needs as many independent active rows as parameters (`expected_active` = 5 for a free direction, 3 for a fixed one). An oval cross-section's near-axis envelope has 4 contacts (free direction) or 2 in projection (fixed direction), so the fit always fails closed (`solver`); fixture cases `oval-free` and `oval-fixed` assert this typed refusal. Open: supporting a degenerate active set in the certifier (`KRN.SOLV.CERT`: certifying a KKT point with fewer active rows than parameters, for example through second-order conditions on the null space), so that oval shafts get an envelope rather than a refusal.
- **Short wide features (reviews of #374).** Short oval pilot diameters (radius 50 mm, 20 um two-lobe ovality, 2.5 mm and 5 mm long: fixture cases `pilot` and `pilot-5mm`) have a certified near-axis strict local minimum that a crosswise or tilted cylinder beats; on `pilot-5mm` four fixed check directions all pass. The cap cover (guard 5) finds a smaller feasible cylinder on both (`better`, at the first and third cap), so the fit is `local_optimum_only` and the trait refuses it. Open: restarting the free fit from the better direction so that such cases return the global envelope rather than a refusal.
- **Slender shafts (review 3 of #374).** Caps near the fit's axis are decided only when `sqrt(R^2 + max |q|^2) h` falls below `tol = 1e-9 R`, so the chord radius `h` needed shrinks with the aspect ratio (length over radius). For aspect ratios above about 1000 this may need more than `MAX_DEPTH = 40` levels, and the cross-check then ends `inconclusive`: the fit is `local_optimum_only` and refused by the trait (fail closed, never a wrong pass). Open: a deeper or adaptive limit, or a local certificate radius around the certified axis, for such shafts.
- **Cost of the global cross-check (measured, release build, x86-64 Windows).** On the fixture's planted envelopes (1 000 and 2 000 points) every cross-check passes at cap depth 30 to 31 with 13 000 to 61 000 cap tests and 44 to 86 full tests, adding about 0.1 to 1.2 s to a fit. One full test (projection, hull, calipers, smallest circle, upper bound) on 200 000 points takes 30 to 41 ms, so a cross-check at 200 000 points is estimated at 2 to 4 s, beyond the unmeasured 500 ms budget [unverified estimate]. When a case needs more than the work limits, it fails closed (`inconclusive`).
- **Partial arcs.** Half cylinders 1.066 radii long and short noisy 30-degree arcs have no unique enclosing cylinder tied to the measured surface (the half circle's endpoints are antipodal, so the active set is degenerate); every such fixture case (two half cylinders, axes 0.01 m and 1 km from the origin; two short arcs; 150 points each) is refused (`solver`: certificate failed or iteration limit), never returned unflagged.
- **Note for `KRN.LSQ.AXIS.CYL` (sibling, wording only).** Its NODE.md cites the review of #362 for "no wrong minimum fell below a ratio of 0.44"; that floor came from a sampled search of the fixture's minima, not an exhaustive one, and the sentence should say "sampled search". The `ls_cyl` re-anchoring now passes numeric solver errors (non-finite values, allocation failures) through instead of keeping the earlier round (fixed in this PR).

## Children

_Leaf: none._
