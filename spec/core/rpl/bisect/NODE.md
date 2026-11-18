---
id: CORE.RPL.BISECT
title: First-divergence search and attribution
level: 3
parent: CORE.RPL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-replay]
built_pr: 127
satisfies: [REQ-CORE.RPL-013]
covers: []
depends_on: [CORE.RPL.CLASS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# CORE.RPL.BISECT: First-divergence search and attribution

## Purpose

When a replay is `DIVERGENT`, tells the reader where and why: it finds the first divergent journal entry (by bisection when only state digests at checkpoints are known, linearly when per-entry outcomes exist) and attributes the divergence to exactly one primary cause: input, parameter, defaults profile, command migration or kernel.

## Scope

**In:** bisection over a monotone divergence predicate, linear first-divergence over per-entry outcomes, attribution rules and precedence, the report fragment written into the Replay Report.
**Out:** producing replayed digests (`CORE.RPL.EXEC`); classifying outcomes (`CORE.RPL.CLASS`); rendering the report (`RPT`, `TRUST`).

## Requirements

- **REQ-CORE.RPL.BISECT-001** When per-entry outcomes are available, the search shall return the lowest-sequence entry whose outcome is `DIVERGENT`.
- **REQ-CORE.RPL.BISECT-002** When only a predicate "state after prefix k diverges" is available, the search shall find the first divergent entry of a span of N entries with at most ceil(log2(N)) + 1 predicate evaluations, assuming the predicate is monotone.
- **REQ-CORE.RPL.BISECT-003** When the first divergent entry is found, the attribution shall compare its recorded and replayed input digests, argument commitments, defaults-profile digest, command major or migrator and kernel manifest digest.
- **REQ-CORE.RPL.BISECT-004** The attribution shall name one primary cause using the precedence input, parameter, defaults profile, command migration, kernel, and shall list every other matching cause as secondary.
- **REQ-CORE.RPL.BISECT-005** If no compared item differs, then the attribution shall name `kernel` as the primary cause with the note `no_input_difference`.
- **REQ-CORE.RPL.BISECT-006** If the predicate is found to be non-monotone during the search, then the search shall report `non_monotone` and fall back to a linear scan.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/rpl/bisect/span_1000.json` (1,000 synthetic entries with a divergence injected at sequence 637), when bisection runs with a counting predicate, then it returns 637 using at most 11 evaluations.
- **AC-2** Given fixture `fixtures/core/rpl/bisect/attribution.json` (five cases, one per cause), when attribution runs, then each case's primary cause matches the expected cause.
- **AC-3** Given a case where both an input digest and the kernel digest differ, then the primary cause is `input` and `kernel` is secondary.
- **AC-4** Given a non-monotone predicate fixture, then the result carries `non_monotone` and the linear fallback returns the correct entry.
- **AC-5** Given the same inputs, when run twice, then the outputs are byte-identical.

## Design

- **Registry command(s):** none; library called by every replay command that reports DIVERGENT.
- **Data structures:** `Attribution { entry_seq: u64, primary: Cause, secondary: Vec<Cause>, notes: Vec<String> }`; `enum Cause { Input, Parameter, DefaultsProfile, CommandMigration, Kernel }`.
- **Algorithm:** classic lower-bound binary search over [0, N) on the predicate, with a monotonicity spot check at the found boundary (predicate(k-1) false, predicate(k) true); attribution by field comparison in the fixed precedence.
- **Errors:** `non_monotone` (reported, not fatal), `empty_span`.
- **Determinism:** D0.
- **Performance budget:** dominated by predicate cost; the search itself is O(log N).
- **Crate:** `fid-replay` (module `bisect`).

## Test fixtures

Synthetic spans and attribution cases under `fixtures/core/rpl/bisect/`, generated from a fixed seed by a committed script; CC-BY-4.0.

## Build prompt

> In crate `fid-replay`, add module `bisect` with `first_divergent(outcomes: &[Outcome]) -> Option<u64>`, `bisect(n: u64, pred: impl FnMut(u64) -> Result<bool, E>) -> Result<BisectResult, E>` (at most ceil(log2 n)+1 calls; check monotonicity at the boundary and fall back to a linear scan with `non_monotone` if it fails), and `attribute(recorded: &EntryFacts, replayed: &EntryFacts) -> Attribution` comparing input digests, argument commitments, defaults-profile digest, command major/migrator and kernel manifest digest, with primary-cause precedence input > parameter > defaults profile > command migration > kernel, and `kernel` plus note `no_input_difference` when nothing differs. Tests: AC-1..AC-5 with fixtures in `fixtures/core/rpl/bisect/`. Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), deterministic output.

## Open questions

- The cause precedence is this leaf's choice; the concept docs list the causes without an order.

## Children

_Leaf: none._
