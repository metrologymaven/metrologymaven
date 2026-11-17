---
id: IO.CORP.GATE
title: Generated fidelity reports and the CI regression gate
level: 3
parent: IO.CORP
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CORP-007, REQ-IO.CORP-008]
covers: []
depends_on: [IO.CORP.SCORE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.CORP.GATE: Generated fidelity reports and the CI regression gate

## Purpose

Regenerates the published per-format fidelity reports on every change to an IO crate or corpus manifest, compares them with the baseline on `main`, and fails CI when any score gets worse unless a recorded waiver approved by a maintainer covers it.

## Scope

**In:**
- CI trigger paths (IO crates, `corpus/`)
- Report generation (JSON plus Markdown) from `import.score_fidelity@1` results; committed reports are generated, never hand-edited
- Regression rule and waiver files (`corpus/waivers/<id>.toml`: format, metric, old, new, reason, approver)
- Waiver approval check: approver is a maintainer listed in IO's `owners:` and is not the change author (D-003)

**Out:**
- Metric computation (`IO.CORP.SCORE`), hosting published reports (ECO, OUTR)

## Requirements

- **REQ-IO.CORP.GATE-001** CI shall regenerate the fidelity reports on every change touching an IO crate or a corpus manifest, and shall fail if committed reports differ from the regenerated ones.
- **REQ-IO.CORP.GATE-002** If a change lowers any format's round-trip score or raises any file's maximum coordinate difference against the `main` baseline, then CI shall fail unless a waiver for that format and metric is present.
- **REQ-IO.CORP.GATE-003** A waiver shall count only if its approver is a listed maintainer other than the change author, per D-003.
- **REQ-IO.CORP.GATE-004** Each generated report shall state the corpus manifest digests and the reader and writer versions it was produced from.

## Acceptance criteria

- **AC-1** Given baseline `fixtures/io/corp/gate/base.json` (STL round-trip 1.0) and new result `worse.json` (0.5), when the gate runs, then it fails naming `mesh.stl roundtrip_score 1.0 -> 0.5`.
- **AC-2** Given the same with `fixtures/io/corp/gate/waiver_ok.toml` (approver `m2`, author `m1`, maintainers `[m1, m2]`), then the gate passes and the report lists the waiver.
- **AC-3** (error path) Given `waiver_self.toml` (approver equals author), then the gate fails `waiver_invalid`.
- **AC-4** Given a hand-edited committed report, then CI fails `report_not_generated`.
- **AC-5** Given equal results, then the gate passes and the regenerated Markdown is byte-identical to `fixtures/io/corp/gate/expected.md`.

## Design

- **Registry commands:** none (CI tool `corpus-gate`).
- **Data structures:** `Baseline` (per format: score, per-file max diff); `Waiver { format, metric, old, new, reason, approver, author }`.
- **Algorithm:** load baseline from `main`'s committed report JSON; compare metric by metric with exact decimal comparison; apply waivers; emit JSON (RFC 8785) and Markdown.
- **Errors:** `regression`, `waiver_invalid`, `report_not_generated`.
- **Determinism:** D0.
- **Performance budget:** trivial beyond scoring.
- **Crate:** `fid-io-corp`.

## Test fixtures

Baseline, result, waiver and expected-report files under `fixtures/io/corp/gate/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-corp`, add CI binary `corpus-gate`: regenerate per-format fidelity reports (JSON RFC 8785 + Markdown, stating manifest digests and reader/writer versions) from `import.score_fidelity@1` results; fail `report_not_generated` if committed reports differ; compare against the `main` baseline and fail `regression` when a round-trip score drops or a max coordinate difference rises, unless a waiver (`corpus/waivers/*.toml`: format, metric, old, new, reason, approver, author) is approved by a listed IO maintainer other than the author (D-003), else `waiver_invalid`. Wire it to run on changes to IO crates or `corpus/`. No panics, no unwrap (CONSTITUTION §8). Tests AC-1..AC-5.

## Open questions

- IO's `owners:` list is empty today; until maintainers are listed, waivers cannot pass.

## Children

_Leaf: none._
