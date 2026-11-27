---
id: RPT.STATS.SPC.RULES
title: Run-rule sets and violation detection
level: 4
parent: RPT.STATS.SPC
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-stats]
built_pr: 668
satisfies: [REQ-RPT.STATS.SPC-005, REQ-RPT.STATS.SPC-007]
covers: []
depends_on: [KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.STATS.SPC.RULES: Run-rule sets and violation detection

## Purpose

Detects out-of-control patterns in a sequence of plotted points relative to a centre line and control limits. Rules are built from a small set of parameterised pattern primitives, and rule sets are named, versioned data. Which rule set is the default, and the exact parameters of the well-known published sets, are expert choices; this leaf ships the engine, a conservative default set (`beyond_limits@1`) and the published sets as data marked `UNVERIFIED`.

## Scope

**In:**
- Pattern primitives and their exact semantics
- Rule-set data format, the shipped sets and their versions
- Kernel function `evaluate_rules(points, lines, set) -> Vec<RuleViolation>`

**Out:**
- Computing limits (`RPT.STATS.SPC.LIM`), choosing frozen limits (`RPT.STATS.SPC.FREEZE`)
- Putting violation markers into chart series (`RPT.STATS.SPC.CHART`)

## Requirements

- **REQ-RPT.STATS.SPC.RULES-001** The engine shall support these primitives, with sigma on each side taken as (UCL − centre)/3 above and (centre − LCL)/3 below: `BEYOND{z}` (one point more than z sigma from the centre), `K_OF_M_BEYOND{k, m, z}` (k of m consecutive points more than z sigma from the centre on the same side), `RUN_SAME_SIDE{len}`, `TREND{len}` (len consecutive strictly increasing or strictly decreasing points), `ALTERNATING{len}` (len consecutive points alternating up and down), `RUN_WITHIN{len, z}` (len consecutive points within z sigma on either side) and `RUN_OUTSIDE{len, z}` (len consecutive points more than z sigma from the centre on either side).
- **REQ-RPT.STATS.SPC.RULES-002** A point exactly on the centre line shall break a same-side run, an equal consecutive value shall break a trend and an alternation, and a point exactly at z sigma shall not count as beyond it [unverified conventions pending expert review].
- **REQ-RPT.STATS.SPC.RULES-003** A rule set shall be data with an ID, a version, a review status and an ordered list of rules, each with a rule ID and one primitive with parameters.
- **REQ-RPT.STATS.SPC.RULES-004** The engine shall ship `beyond_limits@1` (one rule, `BEYOND{3}`, which is the plain control-limit test) as the default set, and the published sets `nelson@1` and `western_electric@1` as data with status `UNVERIFIED` [unverified parameters].
- **REQ-RPT.STATS.SPC.RULES-005** When a pattern is completed at point i, the engine shall emit one `RuleViolation` at point i naming the rule-set ID, version, rule ID and the index range of the pattern; a point may carry violations of several rules.
- **REQ-RPT.STATS.SPC.RULES-006** Violations shall be returned ordered by point index and then by rule position in the set, and the result shall be bit-identical on every supported target (D0).
- **REQ-RPT.STATS.SPC.RULES-007** If a rule-set file is malformed or names an unknown primitive, then loading shall fail with `stats.ruleset_invalid` and no rules shall be evaluated.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/stats/rule_cases.json` (one hand-built sequence per primitive that triggers it exactly once, and one that misses it by one point), when evaluated, then each primitive fires exactly where expected and not on the near-miss.
- **AC-2** (boundaries) Given sequences with points exactly on the centre line, exactly at z sigma and with equal consecutive values, when evaluated, then results follow REQ-002.
- **AC-3** (asymmetric limits) Given an R chart with LCL clamped to 0, when `BEYOND{3}` is evaluated, then the lower sigma is (centre − 0)/3 and only points below 0 or above UCL fire.
- **AC-4** (default set) Given `beyond_limits@1` and fixture `fixtures/rpt/stats/spc_imr_30` with one point above UCL, when evaluated, then exactly one violation is returned.
- **AC-5** (error path) Given a rule-set file naming primitive `MAGIC`, when loaded, then it fails with `stats.ruleset_invalid`.
- **AC-6** (determinism) Given 10 000 points from the `fid-num` RNG with seed 11 and `nelson@1`, when evaluated on x86-64, ARM64 and wasm32, then result digests are identical.

## Design

- **Registry commands:** none (kernel function and data). `side_effect: R1` is that of its caller.
- **Kernel function:** `evaluate_rules(points: &[f64], lines: &Lines, set: &RuleSet) -> Result<Vec<RuleViolation>, StatsError>`.
- **Data structures:** `enum Primitive { Beyond{z}, KOfMBeyond{k, m, z}, RunSameSide{len}, Trend{len}, Alternating{len}, RunWithin{len, z}, RunOutside{len, z} }`; `RuleSet { id, version, review, rules: Vec<Rule{id, primitive}> }`; `RuleViolation { index: u32, set_id, set_version, rule_id, from: u32, to: u32 }`.
- **Shipped data:** `beyond_limits@1`; `western_electric@1` (four zone rules) and `nelson@1` (eight rules), parameters from the published references — Western Electric, Statistical Quality Control Handbook, 1956, and L. S. Nelson, Journal of Quality Technology, 1984 [unverified citation details] — written by the project as parameters only.
- **Algorithm:** one linear pass per rule with sliding counters; thresholds computed once as centre ± z·sigma using plain IEEE operations (no FMA); comparisons with total order.
- **Errors:** `stats.ruleset_invalid{reason}`, `stats.non_finite{index}`.
- **Determinism:** D0.
- **Performance budget:** 100 000 points × 8 rules in under 20 ms [unverified until measured].
- **Crate:** `fid-stats` (kernel graph; KNP-1).

## Test fixtures

- `fixtures/rpt/stats/rule_cases.json`: hand-built sequences, CC-BY-4.0.
- `fixtures/rpt/stats/spc_imr_30`: shared with `RPT.STATS.SPC.LIM`.

## Build prompt

> In crate `fid-stats` (kernel graph, KNP-1: no FMA, total-order comparisons, no std HashMap/HashSet, no I/O, NaN is an error), implement run-rule detection.
>
> **Primitives** (sigma above = (UCL − centre)/3, below = (centre − LCL)/3): `Beyond{z}`, `KOfMBeyond{k, m, z}` (same side), `RunSameSide{len}`, `Trend{len}` (strictly monotone), `Alternating{len}`, `RunWithin{len, z}`, `RunOutside{len, z}`. Conventions: a point on the centre line breaks a same-side run; equal consecutive values break trend and alternation; a point exactly at z sigma is not beyond it.
>
> **Function:** `evaluate_rules(points, lines, set) -> Result<Vec<RuleViolation>, StatsError>`; emit a violation at the point that completes a pattern with rule-set ID, version, rule ID and index range; order by index then rule position.
>
> **Data:** rule sets as embedded canonical JSON: `beyond_limits@1` (default, `Beyond{3}`), `western_electric@1` and `nelson@1` with review status `UNVERIFIED`, parameters only (no copied text). Malformed or unknown primitive -> `stats.ruleset_invalid`.
>
> **Tests:** AC-1 to AC-6 above with `fixtures/rpt/stats/rule_cases.json` and a seeded cross-target digest test.
>
> **Constraints:** CONSTITUTION §8; C1. Spec: `spec/rpt/stats/spc/rules/NODE.md`.

## Open questions

- **Expert choice (open):** the default rule set for production use, the parameters of the published sets, the boundary conventions in REQ-002, and which sets apply to dispersion charts (R, s, MR). Until reviewed, `beyond_limits@1` is the default and the published sets show `UNVERIFIED`.
- Built in `fid-stats` (modules `rules` and `rules_load`; reference `fixtures/rpt/stats/make_rule_cases.py`). Choices made at build time, to confirm or reword in REQ-001 to REQ-007 and the Design: (1) `evaluate_rules` returns a `RuleEvaluation` (the `violations` list plus one outcome per rule, set ID, version, review status and point count) instead of a bare `Vec<RuleViolation>`, so a rule that needs more points than the sequence has is reported as `TooFewPoints{needed}` rather than looking like "in control". (2) A pattern that stays complete fires at every later point that completes it (a run of 10 fires `RUN_SAME_SIDE{9}` twice), following REQ-RPT.STATS.SPC-005 "flag every point"; the range is the minimal pattern ending at that point. (3) `K_OF_M_BEYOND` fires only at a point that is itself beyond on the counted side, and accepts fewer than m points at the start of the sequence when k of them qualify. (4) `z = 3` uses UCL and LCL themselves rather than `centre + 3 · (UCL − centre)/3`, so `BEYOND{3}` is exactly the control-limit test whatever the rounding. (5) Values are normalised with `+ 0.0` before totalOrder comparisons, so a `-0.0` point sits on a `+0.0` centre line. (6) New refusals: `stats.bad_lines{reason}` (non-finite lines, `UCL ≤ centre`, `LCL ≥ centre`, an overflowing span, a half-width below the floor `16 · ε · max|line|`, a rule zone that overflows or rounds onto the centre) and `stats.too_many_points` (over `u32::MAX`); no points is `stats.insufficient_data`. (7) `RuleViolation` also carries the set's review status. (8) `beyond_limits@1` is shipped `UNVERIFIED` too: its rule is definitional, but the REQ-002 conventions it applies are unreviewed. (9) The rule-set file format is `fid.stats.ruleset/1` (see `crates/fid-stats/src/rules_load.rs`): bounded to 64 KiB, nesting depth 8 and 64 rules; duplicate and unknown keys, string escapes and NaN literals are refused; `k`, `m`, `len` up to 1000 and `0 < z ≤ 1000`; a file's `EXPERT_REVIEWED` status is the file's own claim and is not verified by the kernel. (10) The AC-4 fixture is `fixtures/rpt/stats/spc_imr_30.json`; its lines are the individuals-chart limits with E2 = 2.660 from `spc.constants.normal@1.0.0` (`UNVERIFIED`) and `RPT.STATS.SPC.LIM` may extend the file. (11) AC-6 draws each point as the sum of 12 `fid-num` uniforms minus 6 (seed 11, stream 0) with lines 0 ± 3, and checks all three shipped sets. (12) The 100 000 points × 8 rules in 20 ms budget has not been measured.

## Children

_Leaf: none._
