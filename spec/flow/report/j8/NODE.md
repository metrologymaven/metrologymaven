---
id: FLOW.REPORT.J8
title: J8 supplier package intake with tamper detection
level: 3
parent: FLOW.REPORT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.REPORT-001, REQ-FLOW.REPORT-009, REQ-FLOW.REPORT-010, REQ-FLOW.REPORT-011]
covers: [JOB.ACC.08]
depends_on: [TRUST.PKG.EXPORT, TRUST.PKG.READER, TRUST.VERIFY.CLI, TRUST.VERIFY.REPORT, VIZ.OVERLAY.STATUS]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.RUNNER.PKG, ECO.DATA.CAT, TRUST.PKG.FIXTURE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.REPORT.J8: J8 supplier package intake with tamper detection

## Purpose

Owns acceptance job `JOB.ACC.08`: a supplier-quality engineer receives 5 result packages from suppliers, verifies each, sees the correct evaluation-basis badge on every result, catches the one tampered package every time, and records a disposition per package.

## Scope

**In:**
- Synthetic data: 5 result packages built from synthetic projects, 2 holding RESULTS_ONLY results and 1 tampered after export (one byte of a result changed)
- Journey map `JOB.ACC.08`; packages `j8-unassisted` and `j8-assisted`, both tracked

**Out:**
- Package format, verification and report (owned by `TRUST.PKG`, `TRUST.VERIFY`)
- Basis badges (owned by `VIZ.OVERLAY.STATUS`)

## Requirements

- **REQ-FLOW.REPORT.J8-001** The J8 data generator shall produce, from a seed, 5 exported result packages, 2 of them holding only RESULTS_ONLY results, and one copy altered in one result byte after export, with a truth file naming the tampered package.
- **REQ-FLOW.REPORT.J8-002** The J8 journey map shall list inspecting (`verify.inspect_package@1`) and verifying (`verify.package@1`) each package, reviewing results with basis badges, and recording a disposition per package, with owning nodes.
- **REQ-FLOW.REPORT.J8-003** The `j8-unassisted` package shall declare a `verify_verdict` check that the tampered package fails chain verification and the other 4 pass, and the runner shall repeat this check on every nightly run so that detection in 100 % of runs is measured.
- **REQ-FLOW.REPORT.J8-004** The package shall declare a `basis_label` check that every displayed result shows the badge matching its evaluation basis.
- **REQ-FLOW.REPORT.J8-005** The package shall declare a task-time target of under 3 minutes for the 5 packages [unverified] and be declared tracked; `j8-assisted` (agent runs read-only verification and summarises; human records dispositions) shall be tracked.

## Acceptance criteria

- **AC-1** Given seed 8, when the generator runs twice, then the 5 packages and truth file are byte-identical.
- **AC-2** Given the J8 packages, when `fid accept run-suite --job JOB.ACC.08 --json` runs, then the tampered package is reported failed and all checks pass.
- **AC-3** (error path) Given a verifier test build that skips result-digest checks, then `verify_verdict` fails.

## Design

- **Registry command(s):** none; generator `cargo xtask accept-data report --job j8`; content under `acceptance/jobs/j8/`.
- **Data structures:** packages per `acceptance-job@1`; checks `replay_outcome`, `verify_verdict`, `basis_label`.
- **Algorithm:** build 5 synthetic projects, export unsigned packages through the package exporter, alter one byte of one result in a copy.
- **Errors:** generator `DATA-IO`.
- **Determinism:** D0; times observed (N).
- **Performance budget:** verification of 5 packages under 30 s machine time [unverified].
- **Crate:** `fid-accept-data`.

## Test fixtures

Generated packages, CC0, recorded in `datasets.toml`; conformance fixtures of `TRUST.PKG.FIXTURE` as a cross-check.

## Build prompt

> Add `--job j8` to `cargo xtask accept-data report`: build 5 synthetic projects (2 with RESULTS_ONLY results only), export them as result packages, and tamper one copy by changing one result byte; write `truth.json`. Create `acceptance/jobs/j8/` with `map.json` (`JOB.ACC.08`: verify.inspect_package@1, verify.package@1, result review with basis badges, disposition per package) and tracked packages `j8-unassisted` (3 min) and `j8-assisted`; checks replay_outcome, verify_verdict, basis_label. Verify AC-1..AC-3 of spec node FLOW.REPORT.J8.

## Open questions

- No node yet owns a "record disposition of a received package" command; the map names it as a screen step until TRUST or RPT defines it.
- J8 is tracked; changing it to gating is a maintainer decision.

## Children

_Leaf: none._
