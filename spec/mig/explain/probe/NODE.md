---
id: MIG.EXPLAIN.PROBE
title: Scratch-branch probe runner and probe budget
level: 3
parent: MIG.EXPLAIN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.EXPLAIN-002, REQ-MIG.EXPLAIN-003, REQ-MIG.EXPLAIN-004, REQ-MIG.EXPLAIN-005]
covers: []
depends_on: [MIG.EXPLAIN.VARIANT, MIG.PARALLEL.DIFF, CORE.RPL.RETGT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.EXPLAIN.PROBE: Scratch-branch probe runner and probe budget

## Purpose

Executes probe variants for one flagged pair without touching the working document. Each probe runs as a dry-run replay on a throwaway branch (`CORE.RPL.RETGT`), reads the variant's resulting value for the characteristic, and recomputes the difference against the incumbent value with the same difference function the parallel run used (`MIG.PARALLEL.DIFF`). The runner enforces the per-pair probe budget and a fixed probe order, so the list of probes tried is reproducible.

## Scope

**In:**
- Probe plan: the ordered list of single-factor probes (catalogue order, then candidate digest) and, if requested by the caller, two-factor probes for a given lead factor
- Execution of each probe as a `DryRun` replay of the characteristic's evaluation span with the variant's bindings
- Residual difference per probe (signed and absolute, and as a fraction of tolerance) via `MIG.PARALLEL.DIFF`
- Budget accounting: default 12 probes per pair [unverified default], configurable 1..64; probes that return `NOT_APPLICABLE` do not consume budget
- `ProbeResult` records with the variant digest, replay outcome and residual

**Out:**
- Variant construction (owned by `MIG.EXPLAIN.VARIANT`)
- Choosing the lead factor and crediting (owned by `MIG.EXPLAIN.ATTR`)
- Replay machinery and throwaway-branch disposal (owned by `CORE.RPL.RETGT`)

## Requirements

- **REQ-MIG.EXPLAIN.PROBE-001** Every probe shall execute through the replay engine in dry-run mode on a throwaway branch, and after the probe set completes no persistent branch head shall have moved.
- **REQ-MIG.EXPLAIN.PROBE-002** For every executed probe, the runner shall report the variant digest, the replay outcome, our probed value with unit, the signed and absolute residual difference against the incumbent value, and the residual as a fraction of the tolerance zone where one exists.
- **REQ-MIG.EXPLAIN.PROBE-003** The runner shall execute single-factor probes in factor catalogue order and, within a factor, in ascending candidate digest order.
- **REQ-MIG.EXPLAIN.PROBE-004** If executing the next probe would exceed the pair's budget, then the runner shall stop, mark the probe set `BUDGET_EXHAUSTED` and return every probe executed so far.
- **REQ-MIG.EXPLAIN.PROBE-005** If a probe's replay fails, then the runner shall record the probe as `PROBE_FAILED` with the replay error code, count it against the budget and continue with the next probe.
- **REQ-MIG.EXPLAIN.PROBE-006** The same pair, candidate set, budget and kernel version shall produce a byte-identical probe-result list under RFC 8785 canonical JSON.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/mig/explain/probe/cyl-fit/` (a project where our cylinder diameter is least-squares and the incumbent reported maximum-inscribed; difference 6 µm, threshold 1 µm) and a candidate set with a `FIT_METHOD` candidate `max_inscribed`, when the runner executes single-factor probes, then the `FIT_METHOD` probe residual is below 0.1 µm [unverified fixture value] and every branch head digest is unchanged afterwards.
- **AC-2** Given the same project with 20 candidates and budget 5, then exactly 5 probes are executed in catalogue and digest order and the set is `BUDGET_EXHAUSTED`.
- **AC-3** (error path) Given a candidate whose variant references a missing resolver row, then that probe is `PROBE_FAILED` with the replay error code and the next probe still runs.
- **AC-4** Given AC-1 inputs run twice and on x86-64 and ARM64, then the probe-result lists are byte-identical.
- **AC-5** Given AC-1, then the Record Journal of the working branch has no new entries after the probe set.

## Design

- **Registry command(s):** none; library called by `correlate.explain_differences@1` (`MIG.EXPLAIN.ATTR`).
- **Data structures:** `ProbePlan { pair_id, budget, steps: Vec<ProbeStep> }`, `ProbeStep::Single(candidate) | Pair(lead, candidate)`, `ProbeResult { step, variant_digest, outcome: Executed | NotApplicable(reason) | ProbeFailed(code), value?, residual?: Residual }`, `ProbeSet { results, status: Complete | BudgetExhausted }`.
- **Algorithm:** build plan → for each step: `build_variant` (VARIANT) → if not applicable record and continue without consuming budget → else check budget → `Replay::run(DryRun, span)` with bindings → read the characteristic's value from the changeset → `diff(incumbent, probed, tolerance)` from `MIG.PARALLEL.DIFF` → record. Rounding variants skip replay and apply the post rule to the recorded value.
- **Errors:** `probe_pair_not_flagged`, `probe_budget_invalid`; replay errors are captured per probe, not raised.
- **Determinism:** D0 for the runner; the probed values inherit the determinism class of the replayed evaluation commands, which for P1a evaluations is D0 [unverified for minimum-zone fits, which may be D3].
- **Performance budget:** 12 probes on a 10,000-point feature in under 2 s [unverified until measured].
- **Crate:** `fid-mig` (module `explain::probe`).

## Test fixtures

`fixtures/mig/explain/probe/`: synthetic projects generated with a seeded generator (cylinder, plane with datum reference frame, hole pattern), with incumbent-side values computed by the test from known alternative settings; CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `explain::probe` implementing spec node MIG.EXPLAIN.PROBE. Given a flagged `PairedCharacteristic`, a validated `CandidateSet` (module `explain::cat`), a budget (default 12, allowed 1..64) and optional lead factor, build the ordered probe plan (single probes in factor catalogue order then candidate digest; pair probes lead+candidate when requested), build each variant with `explain::variant::build_variant`, execute it with the replay engine in `DryRun` mode on a throwaway branch (never the working document), read the probed value, compute the residual with the parallel-run difference function, and return `ProbeSet` with `Complete` or `BudgetExhausted`. Not-applicable variants do not consume budget; failed replays are `PROBE_FAILED` and do consume budget. Output must be byte-identical under RFC 8785 canonical JSON for identical inputs. Tests for AC-1..AC-5 including a check that no branch head moved and no Record Journal entry was added. No `unwrap`/`expect`/`panic`/`unsafe`; no HashMap.

## Open questions

- None.

## Children

_Leaf: none._
