---
id: CORE.RPL.CLASS
title: Replay outcome classification and decision invariance
level: 3
parent: CORE.RPL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.RPL-006, REQ-CORE.RPL-007, REQ-CORE.RPL-008, REQ-CORE.RPL-009]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# CORE.RPL.CLASS: Replay outcome classification and decision invariance

## Purpose

A pure function that turns a comparison of recorded and replayed outputs into one of `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`, per step and for the whole replay. It applies the determinism policy (bit-identity where the class and platform require it), tolerance equivalence per quantity kind from the REP-1 table, decision invariance (any decision or status change is `DIVERGENT`), and the informative `MARGINAL` annotation. Every replay mode uses it.

## Scope

**In:** per-step classification, aggregation over steps, `MARGINAL` annotation, handling of a missing tolerance entry.
**Out:** the REP-1 equivalence tolerance table itself (owned by `UNC.RDL`; consumed through a trait); producing replayed outputs (`CORE.RPL.EXEC`); locating the first divergence (`CORE.RPL.BISECT`).

## Requirements

- **REQ-CORE.RPL.CLASS-001** When every output digest of a step equals its recorded digest, the classifier shall return `IDENTICAL` for that step.
- **REQ-CORE.RPL.CLASS-002** If any conformance decision or characteristic status of a step differs from the recording, then the classifier shall return `DIVERGENT` for that step regardless of numeric differences.
- **REQ-CORE.RPL.CLASS-003** If a D0, D1 or D2 step replayed with the same kernel manifest digest on the same platform class is not bit-identical, then the classifier shall return `DIVERGENT` for that step.
- **REQ-CORE.RPL.CLASS-004** When a step's differing outputs are each within the tolerance for their quantity kind and no decision or status changed, the classifier shall return `EQUIVALENT`.
- **REQ-CORE.RPL.CLASS-005** If the tolerance table has no entry for a differing output's quantity kind, then the classifier shall return `DIVERGENT` for that step and report `tolerance_missing` with the kind.
- **REQ-CORE.RPL.CLASS-006** When a step input is marked missing, the classifier shall return `NOT_REPLAYABLE` for that step.
- **REQ-CORE.RPL.CLASS-007** The classifier shall aggregate a replay as `DIVERGENT` if any step is `DIVERGENT`, else `NOT_REPLAYABLE` if any step is, else `EQUIVALENT` if any step is, else `IDENTICAL`.
- **REQ-CORE.RPL.CLASS-008** When a replayed value lies within the equivalence tolerance of a decision limit, the classifier shall add a `MARGINAL` annotation and shall not change the outcome.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/classify/cases.json` (at least 40 cases, each with recorded outputs, replayed outputs, determinism class, kernel-digest and platform-class flags, decisions, limits and the expected outcome and annotations), when each case is classified, then every outcome and annotation matches.
- **AC-2** Given a D3 step whose length output differs by less than the fixture table's length tolerance and whose decision is unchanged, then the outcome is `EQUIVALENT`.
- **AC-3** Given the same step with a 1 nm difference that flips `CONFORMS` to `NONCONFORMS`, then the outcome is `DIVERGENT` and the annotation list contains `MARGINAL`.
- **AC-4** Given a D0 step with the same kernel digest and platform class and a last-bit difference, then the outcome is `DIVERGENT`, even though the value is within tolerance.
- **AC-5** Given a replay with step outcomes `[IDENTICAL, EQUIVALENT, NOT_REPLAYABLE]`, then the aggregate is `NOT_REPLAYABLE`; adding one `DIVERGENT` step makes it `DIVERGENT`.
- **AC-6** Given a differing output whose quantity kind is absent from the table, then the outcome is `DIVERGENT` with `tolerance_missing`.

## Design

- **Registry command(s):** none; library used by every replay command.
- **Data structures:** `StepComparison { class: Determinism, same_kernel: bool, same_platform_class: bool, outputs: Vec<(QuantityKind, recorded: f64, replayed: f64, recorded_digest, replayed_digest)>, decisions: Vec<(recorded, replayed)>, limits: Vec<(QuantityKind, f64)>, missing: Option<MissingItem> }`; `Outcome`; `Annotation { Marginal, ToleranceMissing(kind) }`; `trait ToleranceTable { fn tolerance(&self, kind) -> Option<Tolerance> }` with absolute and relative parts.
- **Algorithm:** apply rules in order: missing -> NOT_REPLAYABLE; decision or status change -> DIVERGENT; digests all equal -> IDENTICAL; bit-identity required and not met -> DIVERGENT; any kind without tolerance -> DIVERGENT; all within tolerance -> EQUIVALENT; else DIVERGENT. `MARGINAL` is computed independently of the outcome.
- **Errors:** none at run time; invalid inputs are a typed `comparison_invalid` (mismatched output counts).
- **Determinism:** D0; comparisons only, total-order float comparisons, no NaN accepted.
- **Performance budget:** 1 million steps classified in < 1 s [unverified until measured].
- **Crate:** `fid-replay` (module `classify`).

## Test fixtures

`fixtures/core/rpl/classify/cases.json` with a small stand-in tolerance table `fixtures/core/rpl/classify/tolerances.json` (synthetic values, not the normative REP-1 table); CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `classify` with `classify_step(&StepComparison, &dyn ToleranceTable) -> (Outcome, Vec<Annotation>)` and `aggregate(&[Outcome]) -> Outcome`. Rule order: missing input -> NOT_REPLAYABLE; any decision or status change -> DIVERGENT; all output digests equal -> IDENTICAL; class D0/D1/D2 with same kernel digest and same platform class but not bit-identical -> DIVERGENT; a differing output whose quantity kind has no tolerance -> DIVERGENT plus `tolerance_missing`; all differing outputs within tolerance -> EQUIVALENT; otherwise DIVERGENT. Add `MARGINAL` when a replayed value is within tolerance of a decision limit, without changing the outcome. Aggregate: DIVERGENT > NOT_REPLAYABLE > EQUIVALENT > IDENTICAL. The tolerance table comes through the `ToleranceTable` trait; the normative REP-1 data is supplied elsewhere. Tests: AC-1..AC-6 with fixtures in `fixtures/core/rpl/classify/`. Constraints: CONSTITUTION §4 (outcomes, any decision change is DIVERGENT), §8 (no panic, no unsafe, deny unwrap/expect), NaN is an error.

## Open questions

- Aggregation precedence (DIVERGENT above NOT_REPLAYABLE) is this leaf's choice; the concept docs do not state it.

## Children

_Leaf: none._
