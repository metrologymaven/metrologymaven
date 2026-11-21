---
id: TRUST.REPLAY.COMPARE
title: Replay outcome classification and aggregation
level: 3
parent: TRUST.REPLAY
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY-002, REQ-TRUST.REPLAY-003, REQ-TRUST.REPLAY-004, REQ-TRUST.REPLAY-006]
covers: []
depends_on: [TRUST.PKG.READER]
uses_artifacts: [UNC.RDL.REP, CORE.RPL.CLASS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# TRUST.REPLAY.COMPARE: Replay outcome classification and aggregation

## Purpose

Compares each recomputed result with its packaged ResultRecord and classifies it `IDENTICAL`, `EQUIVALENT` or `DIVERGENT`, then folds result outcomes (including `NOT_REPLAYABLE` from SELECT) into one package outcome. Tolerances for `EQUIVALENT` come from the REP-1 replay equivalence-tolerance table, which UNC.RDL owns and publishes; this leaf only reads it.

## Scope

**In:**
- Field-by-field comparison: value (bit-exact first), uncertainty status and value, decision outcome and rule ID, characteristic status
- Tolerance lookup in the REP-1 table by determinism class and quantity kind
- Result outcome and the first differing field
- Package outcome aggregation (worst wins)
- Loading and validating the REP-1 table file (digest recorded in the report)

**Out:**
- The tolerance values themselves (`UNC.RDL`)
- Recomputation (`TRUST.REPLAY.SANDBOX`)
- Report formatting (`TRUST.VERIFY.REPORT`)

## Requirements

- **REQ-TRUST.REPLAY.COMPARE-001** The comparator shall classify a result `IDENTICAL` when the recomputed value, uncertainty, decision and status are bit-equal to the packaged ones.
- **REQ-TRUST.REPLAY.COMPARE-002** The comparator shall classify a result `EQUIVALENT` only when every numeric difference is within the REP-1 tolerance for the algorithm's determinism class and quantity kind and the decision outcome, rule ID and status are unchanged.
- **REQ-TRUST.REPLAY.COMPARE-003** If the decision outcome, decision rule ID or characteristic status differs, then the comparator shall classify the result `DIVERGENT` regardless of numeric differences.
- **REQ-TRUST.REPLAY.COMPARE-004** If a D0 algorithm's recomputed value is not bit-equal, then the comparator shall classify the result `DIVERGENT` and shall not apply a tolerance.
- **REQ-TRUST.REPLAY.COMPARE-005** If the REP-1 table has no entry for a needed determinism class and quantity kind, then the comparator shall classify a non-identical result `DIVERGENT` with reason `NO_TOLERANCE`.
- **REQ-TRUST.REPLAY.COMPARE-006** The package outcome shall be the worst result outcome in the order `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`, and a package with no results shall be `NOT_REPLAYABLE`.
- **REQ-TRUST.REPLAY.COMPARE-007** The comparator shall record the SHA-256 of the REP-1 table it used in its output.

## Acceptance criteria

- **AC-1** Given comparison cases `fixtures/replay/compare/cases.json` (packaged vs recomputed pairs with expected outcomes) and test table `fixtures/replay/compare/rep1-test.json`, when the comparator runs, then every outcome and first-differing field matches.
- **AC-2** Given a D3 value differing by half the tolerance with the same decision, then `EQUIVALENT`; by twice the tolerance, then `DIVERGENT`.
- **AC-3** (error path) Given a D3 value within tolerance but decision `CONFORMS` changed to `INDETERMINATE`, then `DIVERGENT`.
- **AC-4** (error path) Given a D0 value differing in the last bit, then `DIVERGENT`; given a quantity kind missing from the table, then `DIVERGENT` with `NO_TOLERANCE`.
- **AC-5** Given outcome lists `[IDENTICAL, EQUIVALENT]`, `[EQUIVALENT, NOT_REPLAYABLE]`, `[NOT_REPLAYABLE, DIVERGENT]` and `[]`, then the package outcomes are `EQUIVALENT`, `NOT_REPLAYABLE`, `DIVERGENT`, `NOT_REPLAYABLE`.
- **AC-H1** Hypothesis: the comparator achieves 100 % agreement with the expected outcome and first differing field, with false-`DIVERGENT` rate 0 and false-acceptance rate 0, on `fixtures/replay/compare/cases.json` plus every result of the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version, measured by running the comparator against the labels in `cases.json` and each package's `expect.json` with REP-1 table `rep-1@1.0.0`; falsified if any result is misclassified.
- **AC-H2** Hypothesis: the comparator's field outcomes agree with the `fid-rep` classifier (UNC.RDL.REP, algorithm `rdl.rep.classify@1.0.0`) on 100 % of rows of `fixtures/rdl/rep/cases.csv` at the version pinned with `rep-1@1.0.0`, measured by a differential test feeding each row to both; falsified if any row's outcome or `MARGINAL` annotation differs.
- **AC-H3** Hypothesis: comparator output (outcomes, reasons and recorded REP-1 digest) is byte-identical on the six CI host targets (Windows, Linux, macOS × x86-64, arm64) and wasm32 for the frozen replay conformance corpus `fixtures/replay/corpus/<kernel-version>/` (TRUST.REPLAY.CORPUS) for the first released kernel version, measured by comparing the canonical JSON digests from each target; falsified if any digest differs.

## Design

- **Registry command(s):** none.
- **Data structures:** `Rep1Table` (schema published by UNC.RDL: rows keyed by determinism class and quantity kind with absolute and relative tolerances and unit); `ResultOutcome { record_id, outcome, first_diff: Option<FieldPath>, reason }`; `PackageOutcome`.
- **Algorithm:** compare f64 by bit pattern first; for D1/D2/D3 apply `|a-b| <= abs_tol + rel_tol*|b|` with values in SI units; decisions compared as enums; aggregation by rank.
- **Errors:** `rep1.malformed` when the table fails its schema.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-vreplay` (module `compare`).

## Test fixtures

- `fixtures/replay/compare/*`: synthetic cases and a test REP-1 table clearly marked as test data; the production table comes from UNC.RDL releases.

## Build prompt

> In crate `fid-vreplay`, add module `compare`. Load the REP-1 replay equivalence-tolerance table (JSON, schema owned and published by UNC.RDL; record its SHA-256), compare each recomputed result with its packaged ResultRecord and classify: `IDENTICAL` if value, uncertainty, decision and status are bit-equal; `EQUIVALENT` only if numeric differences are within the table's tolerance for the algorithm's determinism class and quantity kind (D0 never gets a tolerance) and decision outcome, rule ID and status are unchanged; otherwise `DIVERGENT` (reason `NO_TOLERANCE` when no table row applies). Aggregate to a package outcome, worst wins: `DIVERGENT` > `NOT_REPLAYABLE` > `EQUIVALENT` > `IDENTICAL`; empty package is `NOT_REPLAYABLE`. Report the first differing field per result. Tests: the five acceptance criteria with `fixtures/replay/compare/*`. Constraints: CONSTITUTION §4 (any decision change is `DIVERGENT`), §8, D0, total-order float comparisons.

## Open questions

- Resolved 2026-11-21 (maintainer decision): UNC.RDL.REP publishes the REP-1 table keyed by field kind, with determinism class as a context rule (schema `rep-table@1`, crate `fid-rep`). This leaf consumes that table and classifier; the wording "by determinism class and quantity kind" in REQ-002, REQ-005 and the Design means: the determinism class decides whether a tolerance applies, the quantity (field) kind selects the row.
- Resolved 2026-11-21 (maintainer decision, review of #407): D0 is always bit-exact. REQ-UNC.RDL.REP-004 now applies the tolerance only to D1/D2/D3, so this leaf's REQ-004 and `fid-rep` agree and the comparator needs no D0 check of its own.

## Change log

- 2026-11-21 (maintainer decision): **Table keying (maintainer decision, 2026-11-21):** the REP-1 table is keyed by **field kind** only (`length`, `angle`, `dimensionless`, `expanded_uncertainty`, `p_conformance`, `exact`). Determinism class, kernel-manifest identity and platform class are **context rules** applied by the classifier (REQ-003/004), not table keys. Rationale: the bound is a property of the quantity (1 nm is 1 nm whatever produced it), the determinism class only decides *whether* a bound may be used at all, and one key avoids a kind x class matrix whose cells would mostly repeat or be `none`. TRUST.REPLAY.COMPARE consumes this table and classifier.
- 2026-11-21 (maintainer decision): **D0 is always bit-exact (maintainer decision, review of #407):** a D0 field is `IDENTICAL` only when bit-identical and `DIVERGENT` otherwise, whatever the kernel-manifest digests; the REP-1 tolerance applies only to D1/D2 (manifests or platform classes differ) and D3. A cross-version tolerance mode for D0, if ever wanted, comes later as an explicit opt-in. Closes the D0 open question.

## Children

_Leaf: none._
