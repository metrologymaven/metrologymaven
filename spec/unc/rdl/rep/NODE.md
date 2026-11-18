---
id: UNC.RDL.REP
title: Replay equivalence tolerance table (REP-1) and classifier
level: 3
parent: UNC.RDL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-010]
covers: []
depends_on: [KRN]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.RDL.REP: Replay equivalence tolerance table (REP-1) and classifier

## Purpose

Owns the normative Replay Equivalence Policy: a versioned table of tolerances per quantity kind, the rules that combine it with determinism class and kernel identity, and a small dependency-light classifier that `CORE.RPL` and `TRUST.REPLAY` call to label each recomputed field and each package `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`. Ownership was assigned to UNC.RDL by the level-2 review (2026-11-16). One table and one classifier keep the journal replayer and the standalone verifier from disagreeing.

## Scope

**In:**
- Table `rep-1@1.0.0` (values below, taken from the project's concept doc 05 §2.9, not from any standard)
- Field kinds: `length`, `angle`, `dimensionless`, `expanded_uncertainty`, `p_conformance`, `exact` (counts, enums, outcomes, statuses, evidence class, index-set digests)
- Context rules by kernel identity and determinism class
- `MARGINAL` informative annotation near decision boundaries
- Classifier crate `fid-rep` usable by CORE (below UNC in layering) and by the WASM verifier

**Out:**
- Running replays (owned by `CORE.RPL` and `TRUST.REPLAY`)
- Declaring each result field's kind (owned by each algorithm's spec card, for example `UNC.DEC.EVAL` declares outcomes `exact`)

## Requirements

- **REQ-UNC.RDL.REP-001** The table `rep-1@1.0.0` shall define EQUIVALENT bounds: length |Δ| ≤ 1e-9 m; angle |Δ| ≤ 1e-9 rad; dimensionless |Δ| ≤ 1e-9 relative; expanded uncertainty |Δ| ≤ 1e-6 relative; p_conformance |Δ| ≤ 1e-6 absolute; `exact` kinds equal only when identical.
- **REQ-UNC.RDL.REP-002** When both sides of an expanded-uncertainty field are absent (UEC-0), the classifier shall return `IDENTICAL`, and when exactly one side is absent, it shall return `DIVERGENT`.
- **REQ-UNC.RDL.REP-003** When the kernel manifest digests are equal and the step is D0, or D1/D2 on the same platform class, the classifier shall return `IDENTICAL` only for bit-identical values and `DIVERGENT` otherwise, without applying the table.
- **REQ-UNC.RDL.REP-004** When the manifest digests differ, or the step is D3, the classifier shall return `IDENTICAL` for bit-identical values, `EQUIVALENT` within the table bound for the field kind, and `DIVERGENT` beyond it.
- **REQ-UNC.RDL.REP-005** Any difference in a field of kind `exact`, including any conformance outcome or characteristic status, shall be `DIVERGENT` regardless of numeric tolerance.
- **REQ-UNC.RDL.REP-006** The classifier shall decide |Δ| ≤ bound by exact arithmetic (an error-free difference compared with the decimal bound by exact rational comparison), never by a rounded f64 subtraction.
- **REQ-UNC.RDL.REP-007** When a numeric value lies within its field's table bound of a decision limit supplied by the caller, the classifier shall add the informative annotation `MARGINAL` without changing the outcome.
- **REQ-UNC.RDL.REP-008** The package or run outcome shall be the worst field outcome in the order `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`, and a resource trap shall be reported as `NOT_REPLAYABLE(resource)`.
- **REQ-UNC.RDL.REP-009** Any change to a bound or rule shall produce a new table version, and the table version used shall be recorded in every replay report.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/rep/cases.csv` (about 100 rows: field kind, recorded value hex, replayed value hex, same-manifest flag, determinism class, platform-class flag, optional limit, expected outcome and annotation), when classified, then every row matches.
- **AC-2** (boundary) Given length values whose exact difference is 1e-9 m plus one ulp of the operands (row `len_just_over`), then the outcome is `DIVERGENT`; and row `len_just_under`, whose exact difference is just below 1e-9 m, is `EQUIVALENT`.
- **AC-3** (UEC-0) Given U absent on both sides then `IDENTICAL`; absent on one side then `DIVERGENT`.
- **AC-4** (outcome) Given outcomes CONFORMS versus INDETERMINATE with p differing by 1e-7, then `DIVERGENT`.
- **AC-5** (same manifest) Given same manifest, D0, values differing by one ulp, then `DIVERGENT`.
- **AC-6** (aggregate) Given field outcomes {EQUIVALENT, NOT_REPLAYABLE, IDENTICAL}, then the package outcome is `NOT_REPLAYABLE`.
- **AC-7** (portability) AC-1 results are bit-identical on wasm32, x86-64 and ARM64.

## Design

- **Registry command(s):** none; library crate consumed by `CORE.RPL` and `TRUST.REPLAY`. Algorithm ID `rdl.rep.classify@1.0.0`.
- **Data structures:** `FieldKind`, `Ctx { same_manifest: bool, determinism: D0|D1|D2|D3|N, same_platform_class: bool, limit: Option<Decimal> }`, `FieldOutcome { outcome, annotations }`, `Table { version, bounds: ordered list of (FieldKind, Bound{abs|rel, Decimal}) }` embedded as canonical JSON with digest.
- **Algorithm:** bit equality first; context rule (REQ-003/004); exact difference via TwoSum (no FMA) giving a double-double; compare with the decimal bound (scaled by max(|a|, |b|) for relative bounds) using `fid-num` `kdec`; `MARGINAL` when the exact distance to the caller-supplied limit is within the bound. Class-N fields are not classified (the replayer substitutes them).
- **Errors:** `kind_unknown`, `nan_input` (NaN is never a valid recorded value).
- **Determinism:** D0.
- **Performance budget:** 10^6 field classifications in under 200 ms [unverified until measured].
- **Crate:** `fid-rep` (depends only on `fid-num`).

## Test fixtures

`fixtures/rdl/rep/cases.csv`: hand-built boundary cases with exact expected outcomes (project-authored, CC-BY-4.0).

## Build prompt

> Create crate `fid-rep` (depends only on `fid-num`; KNP-1; no panics, no `unwrap`; usable from the WASM verifier) implementing algorithm `rdl.rep.classify@1.0.0` and the embedded table `rep-1@1.0.0`: length 1e-9 m absolute; angle 1e-9 rad absolute; dimensionless 1e-9 relative; expanded uncertainty 1e-6 relative (absent on both sides = IDENTICAL, one side = DIVERGENT); p_conformance 1e-6 absolute; `exact` kinds (counts, enums, outcomes, statuses, evidence class, index-set digests) equal only if identical.
> `classify(kind, recorded, replayed, ctx) -> FieldOutcome`: bit-equal -> IDENTICAL. If same manifest digest and (D0, or D1/D2 on the same platform class) -> otherwise DIVERGENT. If manifests differ or D3 -> EQUIVALENT when |Δ| ≤ bound, else DIVERGENT, where |Δ| is computed exactly (TwoSum, no FMA) and compared to the decimal bound by exact rational comparison (`kdec`; relative bounds scale by max(|a|, |b|)). Any `exact`-kind difference is DIVERGENT. Add annotation `MARGINAL` when the value is within its bound of a caller-supplied decision limit. `combine(outcomes)` returns the worst by DIVERGENT > NOT_REPLAYABLE > EQUIVALENT > IDENTICAL; resource traps are `NOT_REPLAYABLE(resource)`. Expose the table version for replay reports. Table values change only by a new version.
> Tests: every row of `fixtures/rdl/rep/cases.csv`; `len_just_over` boundary; UEC-0 rules; outcome flip DIVERGENT; same-manifest one-ulp DIVERGENT; aggregation; bit-identical results on wasm32, x86-64, ARM64.

## Open questions

- **Gap from research hypotheses (2026-11-17):** the rep-1 tolerance values need ratification by an uncertainty expert before the replay hypotheses can be evaluated against them.
- The table values come from concept doc 05 §2.9 and have not been ratified in the constitution; CONSTITUTION §4 names the outcomes but not the bounds. Reviewers should confirm whether a decision record is wanted to adopt `rep-1@1.0.0` as normative.
- `TRUST.REPLAY` currently expects tolerances keyed by determinism class and says KRN publishes them; this leaf keys bounds by quantity kind and applies determinism class as a context rule. TRUST.REPLAY should reference this node.

## Children

_Leaf: none._
