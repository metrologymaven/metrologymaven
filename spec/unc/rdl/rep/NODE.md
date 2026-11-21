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
built_in: [fid-rep]
built_pr: 407
satisfies: [REQ-UNC.RDL-010]
covers: []
depends_on: [KRN.NUM.KDEC]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
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
- **Table keying:** by field kind only; determinism class, manifest identity and platform class are context rules (see Change log).
- **Table document (`rep-table@1`):** canonical JSON (RFC 8785) `{"algorithm", "bounds": [{"bound": <kdec canonical decimal>, "kind", "mode": "abs"|"rel", "unit"}..., {"kind": "exact", "mode": "exact"}], "schema": "rep-table@1", "version"}`, rows in field-kind order; a relative bound has unit `1`. Embedded as `crates/fid-rep/src/table/rep-1@1.0.0.json`; its SHA-256 is `fid_rep::TABLE_DIGEST`. `Table::new` builds and checks other tables (for example a test table), error `table_invalid`; a kind with no row gives `DIVERGENT` with annotation `NO_TOLERANCE`.
- **Algorithm:** bit equality first; context rule (REQ-003/004); exact difference (built: an exact integer difference of the two dyadic values, equal to TwoSum's double-double but without its overflow case); compare with the decimal bound (scaled by max(|a|, |b|) for relative bounds) using `fid-num` `kdec`; `MARGINAL` when the exact distance to the caller-supplied limit is within the bound. Class-N fields are not classified (the replayer substitutes them).
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
- Resolved 2026-11-21 (maintainer decision): the table is keyed by field kind and determinism class is a context rule (see Change log); TRUST.REPLAY.COMPARE records the same decision. TRUST.REPLAY's own text still says tolerances are keyed by determinism class; a TRUST.REPLAY maintainer should align it.
- `fid-rep` defines its own `Determinism` enum with the `fid_types::Determinism` wire names because the crate may depend only on `fid-num`. A follow-up may switch to `fid-types` (also num layer, zero dependencies).

## Change log

- 2026-11-21 (maintainer decision): `depends_on` narrowed from the KRN split node to the leaf fid-rep uses, `KRN.NUM.KDEC` (fid-rep imports only `fid_num::kdec`; KRN.NUM.ERR and KRN.NUM.RED are not imported). Built in `fid-rep`.
- 2026-11-21 (maintainer decision): **Table keying (maintainer decision, 2026-11-21):** the REP-1 table is keyed by **field kind** only (`length`, `angle`, `dimensionless`, `expanded_uncertainty`, `p_conformance`, `exact`). Determinism class, kernel-manifest identity and platform class are **context rules** applied by the classifier (REQ-003/004), not table keys. Rationale: the bound is a property of the quantity (1 nm is 1 nm whatever produced it), the determinism class only decides *whether* a bound may be used at all, and one key avoids a kind x class matrix whose cells would mostly repeat or be `none`. TRUST.REPLAY.COMPARE consumes this table and classifier.
- 2026-11-21 (build): the exact |Δ| is an integer difference rather than TwoSum (same value, no overflow case); `MARGINAL` tests each present finite value against the limit with the field's bound (relative bounds scaled by the larger present magnitude); same-manifest D1/D2 on a different platform class and class N (when called) use the table; an infinite value that is not bit-equal is `DIVERGENT`; `combine([])` is `NOT_REPLAYABLE`.

## Children

_Leaf: none._
