---
id: ECO.APPS.INSPECT
title: Inspect app composition (scan-to-CAD inspection)
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, ECO.APPS.HANDOFF.RO, VIZ.SHELL.DOCK, VIZ.OVERLAY, IO.MESH, IO.CLOUD, IO.CAD, SCAN.CMP, SCAN.CLOUD, ALN.BFIT, ALN.DATUM, GDT.CHAR, RPT.MODEL, TRUST.PKG]
uses_artifacts: [ECO.DATA.EX, AUTO.CLI.TARGET.APP, CORE.RPL.EXEC, ECO.PARITY.STATUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.INSPECT: Inspect app composition

## Purpose

Defines the Inspect app (D-004): the focused app for scan-to-CAD inspection (import scans and CAD, align, compare surfaces, evaluate dimensions and GD&T, report). This leaf owns its manifest content, workflow defaults and end-to-end acceptance test, not any scan, alignment or evaluation logic: the app crate is a manifest plus a start-up call into shared crates.

## Scope

**In:**
- Inspect app manifest (`app.toml`): enabled namespaces (project, mesh/point-cloud/CAD import, scan processing, alignment, surface comparison, dimensional and GD&T evaluation, reporting, capture), forbidden classes, panels, layout, defaults, status and known gaps
- Default workspace layout and defaults suitable for scan inspection (colour-map ranges, deviation-label defaults)
- The app binary entry point
- End-to-end acceptance workflow on a public scan dataset

**Out** (and which node owns it instead):
- Scan processing and comparison (owned by `SCAN`), alignment (owned by `ALN`), evaluation (owned by `GDT`)
- Viewer and overlays (owned by `VIZ`)
- Manifest schema (owned by `ECO.APPS.MANIFEST.SCHEMA`)
- The example project and its dataset (owned by `ECO.DATA.EX` and `ECO.DATA.CAT`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.INSPECT-001** The Inspect manifest shall declare `forbidden_classes = ["M1"]`, so the loader refuses to start if any enabled command causes physical motion.
- **REQ-ECO.APPS.INSPECT-002** The Inspect manifest shall enable the namespaces `project`, `journal`, `registry`, `settings`, `job`, `import`, `export`, `mesh`, `cloud`, `compare`, `geo`, `construct`, `fit`, `align`, `check`, `gdt`, `decide`, `unc`, `report`, `sign`, `view` and `capture`, which cover the scan-to-CAD workflow (import scan and CAD, align, compare, evaluate, report, export a Result Package).
- **REQ-ECO.APPS.INSPECT-003** When the reference scan-to-CAD example is replayed in the running Inspect app, the digests of its exported comparison results, its ResultRecords and its Result Package statement shall equal those from replaying the same journal headless with the `fid` CLI.
- **REQ-ECO.APPS.INSPECT-004** When the release status report is generated, it shall show no consistency finding for app `inspect`.
- **REQ-ECO.APPS.INSPECT-005** The Inspect manifest shall declare status `alpha` or `beta` with a `status_scope` until every P1b node holding the scan-to-CAD workflow is finished (D-017: Inspect 1.0 after P1b).
- **REQ-ECO.APPS.INSPECT-006** The Inspect defaults shall set the deviation colour-map range to ±0.5 mm with a symmetric scale and the deviation-label precision to 3 decimals in millimetres [unverified as typical shop defaults; the app owner may change them in a PR].

## Acceptance criteria

- **AC-1** Given the Inspect `app.toml` and the release catalogues, when `fid-app-manifest check --manifest crates/fid-app-inspect/app.toml --catalogues <release catalogues>` runs, then it exits 0.
- **AC-2** (error path) Given a test copy of the manifest that adds namespace `cmm` against fixture registry `fixtures/apps/inspect/registry-with-m1.json`, then the check fails with `MAN-FORBIDDEN-CLASS`.
- **AC-3** (end to end) Given example `ex-inspect-scan-to-cad` (its scan and CAD datasets fetched through the dataset tool), when the test starts the Inspect app headless on a copy and runs `fid --app inspect journal replay-exact <journal> --json`, and separately replays headless with `fid`, then the replay outcome is `IDENTICAL` in both and the comparison-result, ResultRecord and package statement digests are equal.
- **AC-4** Given the release `status.json`, then the `inspect` entry has zero consistency findings and a non-empty `status_scope` while its status is not `stable`.
- **AC-5** A source listing of `crates/fid-app-inspect/src/` contains only `main.rs`, under 60 lines.

## Design

- **Registry command(s):** none; the app enables commands and defines none. `side_effect: R0` records that the leaf adds no command.
- **Data structures:** `crates/fid-app-inspect/app.toml`: `id = "inspect"`, `status = "alpha"`, `status_scope` naming the covered workflow part, namespaces as REQ-002, `forbidden_classes = ["M1"]`, panels (element tree, viewport, colour-map legend, comparison inspector, characteristic table, report preview), `layout = "inspect.default"`, defaults as REQ-006, placeholder branding, known gaps for scan-inspection seed rows not yet live.
- **Algorithm:** identical entry point to the other apps: load manifest → refuse on findings (exit 2) → install the shared edit guard → start the shell.
- **Errors:** start-up refusal with the loader's findings.
- **Determinism:** D0 for everything the app computes (all values come from shared commands).
- **Performance budget:** app start under 2 s on the minimum hardware profile [unverified until measured]; viewer frame-rate targets are owned by PLAT.HW.
- **Crate:** `fid-app-inspect` (layer `app`).

## Test fixtures

Example `ex-inspect-scan-to-cad` from ECO.DATA.EX, built on a public scan and CAD dataset admitted through ECO.DATA.CAT (licence recorded there); `fixtures/apps/inspect/registry-with-m1.json` written for this leaf (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, create binary crate `fid-app-inspect` (layer `app`). It holds `app.toml` and `src/main.rs` only. `app.toml` (schema v1 of `fid-app-manifest`): `id = "inspect"`, `status = "alpha"`, a `status_scope`, `namespaces = ["project","journal","registry","settings","job","import","export","mesh","cloud","compare","geo","construct","fit","align","check","gdt","decide","unc","report","sign","view","capture"]`, `forbidden_classes = ["M1"]`, panels, `layout = "inspect.default"`, defaults (colour map ±0.5 mm symmetric, deviation labels 3 decimals in mm), placeholder branding, and known gaps for non-live scan-inspection seed rows. `main.rs` (under 60 lines): load the manifest with `fid-app-manifest` (exit 2 with findings on refusal), install the `fid-app-family` edit guard, start the shell. No domain logic and no registry command definitions. Tests for AC-1..AC-5 of spec node ECO.APPS.INSPECT, including the end-to-end replay of example `ex-inspect-scan-to-cad` in the running app (`fid --app inspect journal replay-exact`) versus headless `fid`, comparing comparison-result, ResultRecord and package digests. Constraints: CONSTITUTION §8; app 1.0 only after P1b (D-017).

## Open questions

- Whether live scanner acquisition (HAL, D1) is enabled in Inspect or kept for a later portable workflow is an app-owner choice once HAL scanner drivers exist; adding `device` would not conflict with `forbidden_classes = ["M1"]` as long as no enabled command is M1.

## Children

_Leaf: none._
