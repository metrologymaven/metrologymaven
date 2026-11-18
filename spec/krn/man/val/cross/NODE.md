---
id: KRN.MAN.VAL.CROSS
title: Out-of-kernel cross-check solvers in CI
level: 4
parent: KRN.MAN.VAL
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN.VAL-005]
covers: []
depends_on: [KRN.MAN.VAL.RUN]
uses_artifacts: [UNC.RDL.FREEZE, KRN.MAN.WASM]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.MAN.VAL.CROSS: Out-of-kernel cross-check solvers in CI

## Purpose

An independent second opinion on the kernel's minimum-zone and least-squares answers. CI solves the same reference and synthetic cases with an independent solver stack (for example an open-source LP solver for the linear minimum-zone formulations and an independent nonlinear least-squares library) and fails when the kernel and the independent solver disagree beyond a tolerance. These solvers never link into the kernel.

## Scope

**In:** a cross-check runner in the CI tool crate; adapters that pose the plane and 2D line minimum-zone problems as LPs and the least-squares fits as nonlinear least squares for the independent stack; per-problem tolerances as versioned data; comparison against the kernel results recorded by `KRN.MAN.VAL.RUN`; a dependency-graph check that the independent solvers do not appear in the kernel graph; licence check of the independent solvers against the project allowlist.
**Out:** the kernel algorithms; the main harness (`KRN.MAN.VAL.RUN`); in-kernel combinatorial cross-checks (`KRN.MZ.VOR`, `KRN.ENV.XCHK`).

## Requirements

- **REQ-KRN.MAN.VAL.CROSS-001** For each cross-checked (algorithm, case), the runner shall solve the case with the independent solver and compare the result with the kernel's under the tolerance held as data for that problem type.
- **REQ-KRN.MAN.VAL.CROSS-002** If the two results differ beyond the tolerance, then CI shall fail and report the case identifier, the algorithm ID and both results.
- **REQ-KRN.MAN.VAL.CROSS-003** CI shall fail if any independent solver crate or library appears in the dependency graph of the kernel artifact.
- **REQ-KRN.MAN.VAL.CROSS-004** The independent solvers shall be on the project licence allowlist, and the runner shall run only in CI, never in a shipped binary.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/cross-toy-v1` (20 plane and line minimum-zone cases with kernel results), when the runner runs, then all pass; with one kernel result perturbed by 10 times the tolerance, CI fails naming that case.
- **AC-2** Given a test branch that adds the independent LP solver as a dependency of `fid-fit`, then the graph check fails.
- **AC-3** Given the cross-check job, when run twice on the same inputs, then pass or fail outcomes are identical (results may differ in the last bits, within tolerance).
- **AC-H1** Hypothesis: for the minimum-zone plane problem posed as a linear programme, the independent open-source LP solver agrees with the kernel zone width within 0.001 µm [unverified] on 100% of the `mz.plane` cases (`UNC.RDL.GEN.MZEQ`, generator version as recorded) in the frozen RDL release named in the kernel manifest (`rdl-release-<semver>`, identified by its `rdl_suite_digest`), measured by the runner's per-case difference; falsified if any `Disagreement{case, algorithm}` is raised. The 2D-line minimum-zone problem has no public reference family yet (no `mz.line` generator); until one exists it is tested only on `fixtures/man/cross-toy-v1`.
- **AC-H2** Hypothesis: the independent nonlinear least-squares library agrees with the kernel least-squares parameters within 0.01 µm on position and radius and 0.01 µrad on orientation [unverified] on 100% of the `ls.*` cases (`UNC.RDL.GEN.LSNULL`) and the NIST Algorithm Testing System least-squares cases (`UNC.RDL.EXT.NIST`, source-file digests as recorded) in the frozen release, measured as in AC-H1 after the same representation normalisation as `KRN.MAN.VAL.RUN`; falsified if any disagreement exceeds the tolerance.
- **AC-H3** Hypothesis: the cross-check is not vacuous because the independent stack is itself accurate: its results agree with the reference outputs (the generators' 50-digit references, or the NIST published values) within the same tolerances on 100% of the cases used in AC-H1 and AC-H2, measured by comparing the independent results with the reference outputs; falsified if any case lies outside the tolerance, in which case that case's cross-check is reported as uninformative rather than as agreement.

## Design

- **Registry command(s):** none; CI job.
- **Algorithm:** pose and solve with the independent stack; compare with tolerance; graph check over `cargo metadata` of the kernel artifact.
- **Errors:** `Disagreement{case, algorithm}`, `ForbiddenDependency{crate}`, `LicenceNotAllowed`.
- **Determinism:** D3 (the independent solver is not bound by KNP-1; agreement is judged by tolerance).
- **Performance budget:** < 10 minutes per CI run [unverified until measured].
- **Crate:** `fid-val` (module `cross`; CI tool).

## Test fixtures

`fixtures/man/cross-toy-v1`: synthetic cases with kernel results, CC-BY-4.0.

## Build prompt

> In the CI tool crate `fid-val` (outside the kernel graph; never shipped; no `unsafe`), add module `cross`: pose plane and 2D-line minimum-zone cases as LPs and least-squares fits as nonlinear least squares for an independent, allowlisted open-source solver stack; compare with the kernel results from `run` under per-problem tolerances held as data; fail with `Disagreement{case, algorithm}` and both results on mismatch; add a graph check that the independent solvers never appear in the kernel artifact's dependency graph (`ForbiddenDependency{crate}`) and a licence allowlist check. Tests: `fixtures/man/cross-toy-v1` pass and perturbed-fail cases, forbidden-dependency test branch, repeat-run outcome identity. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-3 of spec node KRN.MAN.VAL.CROSS.

## Open questions

- Which independent solvers to use depends on the licence allowlist set by D-001; candidates must be checked against it before the leaf is built [unverified].

## Children

_Leaf: none._
