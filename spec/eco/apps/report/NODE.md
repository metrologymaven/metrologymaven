---
id: ECO.APPS.REPORT
title: Report app composition (reporting and SPC)
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, ECO.APPS.HANDOFF.RO, VIZ.SHELL.DOCK, RPT.MODEL, RPT.OUTPUT, RPT.BALLOON, IO.RES, GDT.CHAR, UNC.DEC, TRUST.PKG]
uses_artifacts: [ECO.DATA.EX, AUTO.CLI.TARGET.APP, CORE.RPL.EXEC, ECO.PARITY.STATUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.REPORT: Report app composition

## Purpose

Defines the Report app (D-004): the focused app for transcription and inspection reports, result review and multi-part statistics (trend, SPC, capability). It is the first app to ship because the P1a product (transcription reports, prismatic GD&T subset, minimal GUI) is a reporting workflow. This leaf owns the app's manifest content, its app-level defaults and its end-to-end acceptance test, not any reporting logic: the app crate is a manifest plus a start-up call into shared crates.

## Scope

**In:**
- Report app manifest (`app.toml`): enabled namespaces, forbidden classes, panels, layout, templates, defaults, status and known gaps
- Default workspace layout, default report templates and default settings for reporting
- The app binary entry point: load the manifest, install the shared edit guard, start the shell
- Acceptance test: the P1a transcription-report workflow runs end to end in the app and matches the CLI

**Out** (and which node owns it instead):
- Report designer, templates' engine and SPC computations (owned by `RPT`)
- GD&T evaluation (owned by `GDT`)
- Manifest schema and loader (owned by `ECO.APPS.MANIFEST.SCHEMA`)
- The example project and its expected outcome (owned by `ECO.DATA.EX`)
- Other apps (owned by `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.REPORT-001** The Report manifest shall declare `forbidden_classes = ["D1", "M1"]`, so the loader refuses to start if any enabled command is device I/O or motion.
- **REQ-ECO.APPS.REPORT-002** The Report manifest shall enable exactly the namespaces `project`, `journal`, `registry`, `settings`, `job`, `import`, `export`, `report`, `check`, `gdt`, `decide`, `unc`, `sign`, `view` and `capture`, which cover the P1a transcription-report workflow (create project, enter or import results, evaluate characteristics, generate and export a report, export a Result Package); adding a namespace needs a PR that states which workflow step needs it.
- **REQ-ECO.APPS.REPORT-003** When the reference transcription-report example is replayed in the running Report app, the resolved-report digest and the Result Package statement digest shall equal those from replaying the same journal headless with the `fid` CLI.
- **REQ-ECO.APPS.REPORT-004** The Report manifest shall declare status `alpha` or `beta` with a `status_scope` until every P1b node holding the reporting and SPC workflow is finished (D-017: Report 1.0 after P1b).
- **REQ-ECO.APPS.REPORT-005** When the release status report is generated, it shall show no consistency finding for app `report` (every in-scope seed row that is not live is a declared known gap, and no declared gap is live).
- **REQ-ECO.APPS.REPORT-006** The app crate shall contain no source file other than the binary entry point and tests, and the entry point shall only load the manifest, install the shared edit guard and start the shell.

## Acceptance criteria

- **AC-1** Given the Report `app.toml` and the release registry manifest, when `fid-app-manifest check --manifest crates/fid-app-report/app.toml --catalogues <release catalogues>` runs, then it exits 0.
- **AC-2** (error path) Given a test copy of the manifest that adds namespace `device` against fixture registry `fixtures/apps/report/registry-with-d1.json`, when the check runs, then it fails with `MAN-FORBIDDEN-CLASS`.
- **AC-3** (end to end) Given example `ex-report-transcription` from the example index, when the test starts the Report app headless on a copy, runs `fid --app report journal replay-exact <journal> --json`, then `fid --app report report generate-report` and `fid --app report sign export-package` as recorded in the example, and does the same with headless `fid`, then the resolved-report digests and the package statement digests are equal and the replay outcome is `IDENTICAL`.
- **AC-4** Given the release `status.json`, then the `report` entry has zero consistency findings and status `alpha` or `beta` with a non-empty `status_scope`.
- **AC-5** A source listing of `crates/fid-app-report/src/` contains only `main.rs`, and `main.rs` has under 60 lines.

## Design

- **Registry command(s):** none; the app enables commands, it defines none (ECO.APPS.POLICY). `side_effect: R0` records that the leaf adds no command.
- **Data structures:** `crates/fid-app-report/app.toml`: `id = "report"`, `status = "alpha"`, `status_scope = "transcription reports and prismatic GD&T subset (P1a)"`, namespaces as REQ-002, `forbidden_classes = ["D1", "M1"]`, panels (element tree, characteristic table, report preview, results inspector, proposal inbox), `layout = "report.default"`, templates (the P1a transcription and characteristic-list templates from RPT), defaults (report units and decimal places, default decision-rule profile), placeholder branding, known gaps (SPC and designer seed rows until RPT.STATS and RPT.DESIGN are live). Exact panel, layout and template IDs are taken from the catalogues when VIZ.SHELL.DOCK and RPT.MODEL publish them.
- **Algorithm:** `main`: load manifest with the in-process registry and catalogues → on findings, print and exit 2 → install the shared edit guard (ECO.APPS.HANDOFF.RO) → start the shell with the manifest's panels and layout.
- **Errors:** start-up refusal with the loader's findings.
- **Determinism:** D0 for everything the app computes (all values come from shared commands); the E2E test asserts digest equality.
- **Performance budget:** app start to interactive shell under 2 s on the minimum hardware profile [unverified until measured].
- **Crate:** `fid-app-report` (layer `app`).

## Test fixtures

The example `ex-report-transcription` from ECO.DATA.EX (fetched through the dataset tool), and `fixtures/apps/report/registry-with-d1.json` written for this leaf (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, create binary crate `fid-app-report` (layer `app` in `layers.toml`). It holds `app.toml` and `src/main.rs` only. `app.toml` (schema v1 of `fid-app-manifest`): `id = "report"`, `status = "alpha"`, `status_scope = "transcription reports and prismatic GD&T subset (P1a)"`, `namespaces = ["project","journal","registry","settings","job","import","export","report","check","gdt","decide","unc","sign","view","capture"]`, `forbidden_classes = ["D1","M1"]`, panels, `layout = "report.default"`, the P1a templates, report defaults, placeholder branding (`internal = true`), and known gaps for SPC and report-designer seed rows. `main.rs` (under 60 lines): load the manifest with `fid-app-manifest` against the in-process registry and catalogues (print findings and exit 2 on refusal), install the `fid-app-family` edit guard, start the shell. No domain logic, no registry command definitions (CONSTITUTION §2: apps hold policy, shared crates hold logic). Tests: AC-1..AC-5 of spec node ECO.APPS.REPORT, including the end-to-end test that replays example `ex-report-transcription` in the running app (`fid --app report ...`) and headless, and compares the resolved-report and Result Package statement digests. Constraints: CONSTITUTION §8; app 1.0 only after P1b (D-017).

## Open questions

- Whether SPC (P1b) ships inside the Report app or later as an option of it is an app-owner choice; until `stats` commands exist the namespace is left out and SPC rows are known gaps.
- Phase P1a follows the CONSTITUTION §7 P1a theme (transcription reports) and D-017.

## Children

_Leaf: none._
