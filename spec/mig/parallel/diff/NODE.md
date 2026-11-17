---
id: MIG.PARALLEL.DIFF
title: Per-pair difference, tolerance fraction and flag rule
level: 3
parent: MIG.PARALLEL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.PARALLEL-002, REQ-MIG.PARALLEL-003, REQ-MIG.PARALLEL-004]
covers: []
depends_on: [KRN.NUM, GDT.CHAR.RECORD, IO.RES.TRANS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PARALLEL.DIFF: Per-pair difference, tolerance fraction and flag rule

## Purpose

The one function that says how far apart two results are and whether that matters. For a pair it converts both values to the characteristic's SI quantity, computes the signed difference (ours minus incumbent), expresses it as a fraction of the tolerance zone width, compares the decisions, and applies the flag rule. Both the parallel run and the explainer's probes call this same function, so a residual after a probe is measured exactly like the original difference.

## Scope

**In:**
- Value normalisation: the incumbent value from its authored decimal and unit to SI with one rounding (the transcription record's conversion); our value already in SI
- Signed and absolute difference; tolerance zone width from the characteristic (upper minus lower limit, or the geometric tolerance value)
- Decision comparison and decision-change status
- Flag rule with threshold in (0, 1], default 0.10 [unverified default]
- `NO_TOLERANCE` and `INCOMPARABLE` (quantity kind mismatch) handling

**Out:**
- Pairing (owned by `MIG.PARALLEL.PAIR`)
- Report assembly (owned by `MIG.PARALLEL.REPORT`)
- Unit conversion rules themselves (owned by `IO.REG.PROV` and `CORE.BUS.UNIT`)

## Requirements

- **REQ-MIG.PARALLEL.DIFF-001** For a pair, the function shall return both values with units, the signed difference (ours minus incumbent) in the characteristic's SI unit, the absolute difference, and the absolute difference divided by the tolerance zone width.
- **REQ-MIG.PARALLEL.DIFF-002** The function shall set flag reason `OVER_THRESHOLD` when the absolute difference is greater than the threshold times the zone width.
- **REQ-MIG.PARALLEL.DIFF-003** The function shall set flag reason `DECISION_CHANGE` when both decisions are `CONFORMS` or `NONCONFORMS` and differ, and `DECISION_UNDECIDED` when exactly one side is `CONFORMS` or `NONCONFORMS` and the other is not.
- **REQ-MIG.PARALLEL.DIFF-004** If the characteristic has no finite tolerance zone width greater than zero, then the function shall return the absolute difference only, mark the pair `NO_TOLERANCE` and set flag reason `NO_TOLERANCE`.
- **REQ-MIG.PARALLEL.DIFF-005** If the two values have different quantity kinds, then the function shall mark the pair `INCOMPARABLE`, set flag reason `INCOMPARABLE` and return no difference.
- **REQ-MIG.PARALLEL.DIFF-006** If the threshold is not in (0, 1] or a value is non-finite, then the function shall fail with `diff_invalid_input`.
- **REQ-MIG.PARALLEL.DIFF-007** All arithmetic shall use the `fid-num` facade without fused multiply-add, and identical inputs shall give bit-identical results on every supported target.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/parallel/diff/cases.json` (40 hand-computed cases: bilateral, unilateral, geometric, inch-authored incumbent values, decision combinations), when the function runs on each, then every output equals the expected value bit for bit and every flag reason matches.
- **AC-2** Given a case with difference exactly equal to threshold times width, then no `OVER_THRESHOLD` flag is set.
- **AC-3** Given a flatness characteristic with tolerance 0, then the pair is `NO_TOLERANCE` and flagged.
- **AC-4** (error path) Given threshold 0, then the function fails with `diff_invalid_input`.
- **AC-5** Given AC-1 inputs on x86-64, ARM64 and wasm32, then outputs are bit-identical.

## Design

- **Registry command(s):** none; pure function `diff(pair: PairInputs, threshold: f64) -> Result<PairDiff, DiffError>` used by `MIG.PARALLEL.REPORT` and `MIG.EXPLAIN.PROBE`.
- **Data structures:** `PairInputs { incumbent: TranscribedValue, ours: ResultValue, zone: Option<ZoneWidth>, incumbent_decision, our_decision }`; `PairDiff { incumbent_si, ours_si, signed, abs, fraction?: f64, status: Compared | NoTolerance | Incomparable, flags: BTreeSet<FlagReason> }`.
- **Algorithm:** check finiteness and threshold → quantity-kind check → `signed = ours - incumbent` (plain f64 subtraction, no FMA) → `abs` → `fraction = abs / width` → flags. Strict greater-than for the threshold.
- **Errors:** `diff_invalid_input`.
- **Determinism:** D0.
- **Performance budget:** 1 M pairs in under 100 ms [unverified until measured].
- **Crate:** `fid-mig` (module `parallel::diff`), compiled under the KNP-1 lint profile.

## Test fixtures

`fixtures/mig/parallel/diff/cases.json`: hand-computed cases written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `parallel::diff` implementing spec node MIG.PARALLEL.DIFF as a pure function `diff(pair, threshold) -> Result<PairDiff, DiffError>`. Normalise the incumbent value to SI using the transcription record's single-rounding conversion; signed = ours − incumbent; fraction = |signed| / zone width; flags: `OVER_THRESHOLD` if |signed| > threshold × width (strict), `DECISION_CHANGE` when both sides decided (CONFORMS/NONCONFORMS) and differ, `DECISION_UNDECIDED` when exactly one side is decided, `NO_TOLERANCE` when there is no finite positive zone width (absolute difference only), `INCOMPARABLE` when quantity kinds differ (no difference). Fail `diff_invalid_input` for threshold outside (0, 1] or non-finite values. Use the `fid-num` facade, no FMA, no HashSet (BTreeSet for flags); bit-identical across x86-64, ARM64 and wasm32. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
