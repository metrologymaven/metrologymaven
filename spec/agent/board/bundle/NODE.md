---
id: AGENT.BOARD.BUNDLE
title: Static scoreboard bundle
level: 3
parent: AGENT.BOARD
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.BOARD-001, REQ-AGENT.BOARD-004, REQ-AGENT.BOARD-006]
covers: []
depends_on: [AGENT.BOARD.RECOMP]
uses_artifacts: [AGENT.HARNESS.RECORD, AGENT.REDTEAM.VERDICT, AGENT.EVAL.FORMAT]
side_effect: W1
determinism: D0
cli: [fid-eval board build]
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.BOARD.BUNDLE: Static scoreboard bundle

## Purpose

Builds the published scoreboard for a release: a self-contained static folder (data plus a page) that shows, per model, results per task, family and app tag, always next to that model's red-team results and the release's red-team verdict, with all run records included so anyone can recompute it. It builds only from inputs that pass the recompute check.

## Scope

**In:**
- `fid-eval board build --input <dir> --release <name> --out <dir>`
- `board.json` (v1) data file and a static HTML page with no scripts loaded from outside the bundle
- Per-family and per-app aggregation views

**Out** (and which node owns it instead):
- Recompute and submissions (`AGENT.BOARD.RECOMP`)
- Claim register (`AGENT.BOARD.CLAIMS`)
- Hosting on the website (`OUTR`)

## Requirements

- **REQ-AGENT.BOARD.BUNDLE-001** The build shall refuse to write a bundle unless `fid-eval board recompute` passes on the same input, and unless every suite report in it is `publishable: true`.
- **REQ-AGENT.BOARD.BUNDLE-002** For each release and model, `board.json` shall give results per task, per red-team and eval family and per app tag, with suite version, harness version, adapter, declared model (untrusted text), declared settings, run count, run date range and submitter (project or third party).
- **REQ-AGENT.BOARD.BUNDLE-003** Every view that shows a model's capability figures shall show in the same row or card that model's red-team violation and attempt counts and the release's red-team verdict outcome.
- **REQ-AGENT.BOARD.BUNDLE-004** The bundle shall contain no single composite score, and no figure that combines capability with safety into one number.
- **REQ-AGENT.BOARD.BUNDLE-005** The bundle shall include all run records and artifact manifests it was built from, and shall open from the local file system with no network request, account or tracking.
- **REQ-AGENT.BOARD.BUNDLE-006** Building twice from the same input shall give byte-identical `board.json`.

## Acceptance criteria

- **AC-1** Given `fixtures/agent/board/input-ok/` and verdict `fixtures/agent/board/verdict-pass.json`, when `fid-eval board build` runs, then `board.json` equals the golden file and the page lists each model with its red-team counts and verdict on the same card.
- **AC-2** (error path) Given input that fails recompute, then the build fails with `recompute_failed` and writes nothing.
- **AC-3** Given the built page opened in a headless browser with network disabled, then it renders all tables and makes zero network requests.
- **AC-4** Given a scan of `board.json` keys and page text, then no field named or labelled as an overall or composite score exists.
- **AC-5** Given AC-1 twice, then `board.json` is byte-identical.

## Design

- **Registry command(s):** none (D-032). Writes only to the given local output folder.
- **Data structures:** `Board { release, verdict: VerdictRef, models: Vec<ModelEntry { model: Untrusted, adapter, settings, submitter, suite_version, harness_version, runs, date_range, by_task, by_family, by_app, redteam: { attempts, violations } }> }`.
- **Algorithm:** run recompute in-process; load reports and verdict; group; render HTML from a fixed template with inline CSS and no external resources; untrusted strings HTML-escaped.
- **Errors:** `recompute_failed`, `not_publishable`, `verdict_missing`, `release_mismatch`.
- **Determinism:** D0.
- **Performance budget:** < 30 s for 50 models x 500 tasks [unverified].
- **Crate:** `fid-eval-board`

## Test fixtures

Project-authored inputs and verdicts under `fixtures/agent/board/`; golden `board.json`.

## Build prompt

> In crate `fid-eval-board` (Tooling), implement `fid-eval board build --input <dir> --release <name> --out <dir>`. Refuse unless the recompute check passes and every suite report is publishable. Write `board.json` v1 (canonical JSON): per model, results per task, per family and per app tag, with suite and harness versions, adapter, declared model and settings (untrusted, HTML-escaped when rendered), run counts and dates, submitter; and on every model entry its red-team attempts and violations plus the release verdict outcome. Never emit a composite score. Copy all run records and artifact manifests into the bundle. Render a static HTML page from a fixed template with inline CSS, no external requests, no tracking. Acceptance: AC-1 to AC-5 of AGENT.BOARD.BUNDLE. Constraints: CONSTITUTION C7 and §8.

## Open questions

- Which hosted models the project runs for the board depends on funding and on provider terms that may restrict publishing benchmark results [unverified]; until settled, the board lists open-weight models run in project CI and labelled third-party submissions.

## Children

_Leaf: none._
