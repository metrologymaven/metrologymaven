---
id: ECO.APPS.PROBE
title: Probe app composition (CMM programming and execution)
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, ECO.APPS.HANDOFF.RO, VIZ.SHELL.DOCK, IO.CAD, IO.DMIS, CMM.GEN, CMM.PATH, CMM.SIM, CMM.EXEC, CMM.PROBE, HAL.IPP, ALN.DATUM, GDT.CHAR, RPT.MODEL, TRUST.PKG]
uses_artifacts: [ECO.DATA.EX, AUTO.CLI.TARGET.APP, AUTO.MCP, CORE.RPL.EXEC, HAL.SIM.IPPSIM, ECO.PARITY.STATUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.PROBE: Probe app composition

## Purpose

Defines the Probe app (D-004): the focused app for CMM programming, simulation and execution over I++ DME. This leaf owns its manifest content, its defaults and its end-to-end acceptance test against the simulator. Motion authorization stays with CMM and the approval station (D-008); the app only selects what is shown, and its manifest has no way to change how any command is projected to MCP.

## Scope

**In:**
- Probe app manifest (`app.toml`): enabled namespaces (project, CAD and DMIS import, probe and feature programming, path planning, simulation, device status, run hold, evaluation, reporting), panels, layout, defaults, status and known gaps
- Default workspace layout and defaults for CMM programming
- The app binary entry point
- End-to-end acceptance workflow on the I++ simulator, and the MCP projection check

**Out** (and which node owns it instead):
- CMM programming, simulation, execution and the approval station (owned by `CMM`)
- Device drivers and the simulator (owned by `HAL`)
- MCP projection rules (owned by `AUTO.MCP` and `CORE.BUS`)
- Manifest schema (owned by `ECO.APPS.MANIFEST.SCHEMA`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.NETWORK`)

## Requirements

- **REQ-ECO.APPS.PROBE-001** When `fid mcp --bridge` is attached to a running Probe app, the listed MCP tools shall equal the registry-generated projection of the commands in Probe's enabled namespaces, so no M1 command appears and agents keep only `run:hold` and read-only status as motion-related tools (D-008).
- **REQ-ECO.APPS.PROBE-002** The Probe manifest shall enable the namespaces `project`, `journal`, `registry`, `settings`, `job`, `import`, `export`, `geo`, `construct`, `fit`, `cmm`, `device`, `align`, `check`, `gdt`, `decide`, `unc`, `report`, `sign`, `view` and `capture`, which cover programming, simulating and evaluating a prismatic part on the I++ simulator.
- **REQ-ECO.APPS.PROBE-003** When the reference CMM example is replayed in the running Probe app against the I++ simulator, its ResultRecord and Result Package statement digests shall equal those from replaying the same journal headless with the `fid` CLI against the simulator.
- **REQ-ECO.APPS.PROBE-004** When the release status report is generated, it shall show no consistency finding for app `probe`.
- **REQ-ECO.APPS.PROBE-005** The Probe manifest shall declare status `alpha` or `beta` with a `status_scope` until every P2 node holding the CMM programming and execution workflow is finished; an offline programming-and-simulation subset released earlier shall be labelled by that scope (D-017).

## Acceptance criteria

- **AC-1** Given the Probe `app.toml` and the release catalogues, when `fid-app-manifest check --manifest crates/fid-app-probe/app.toml --catalogues <release catalogues>` runs, then it exits 0.
- **AC-2** (projection) Given the Probe app running headless with the simulator, when `fid mcp --bridge` lists tools, then the list equals the list generated from the release registry manifest filtered to Probe's namespaces, and contains no tool whose command class is M1 or A0.
- **AC-3** (end to end) Given example `ex-probe-prismatic-sim` and the I++ simulator started by the test, when the test runs `fid --app probe journal replay-exact <journal> --json` and the same replay headless with `fid`, then both outcomes are `IDENTICAL` and the ResultRecord and package statement digests are equal.
- **AC-4** Given the release `status.json`, then the `probe` entry has zero consistency findings and a non-empty `status_scope` while not `stable`.
- **AC-5** A source listing of `crates/fid-app-probe/src/` contains only `main.rs`, under 60 lines.

## Design

- **Registry command(s):** none; the app enables commands and defines none. `side_effect: R0` records that the leaf adds no command. The manifest schema has no key that changes class or projection (ECO.APPS.MANIFEST.SCHEMA REQ-001), which is how REQ-001 holds by construction; AC-2 checks it end to end.
- **Data structures:** `crates/fid-app-probe/app.toml`: `id = "probe"`, `status = "alpha"`, `status_scope` (for example "offline programming and simulation"), namespaces as REQ-002, no `forbidden_classes` (M1 commands are enabled for humans at the approval station; their projection is fixed by the registry), panels (program tree, viewport with machine model, simulation timeline, device status, characteristic table), `layout = "probe.default"`, defaults (clearance distance, approach and retract distances, default probe build), placeholder branding, known gaps for CMM seed rows not yet live.
- **Algorithm:** identical entry point to the other apps: load manifest → refuse on findings (exit 2) → install the shared edit guard → start the shell.
- **Errors:** start-up refusal with the loader's findings.
- **Determinism:** D0 for computed values; simulator runs are seeded (HAL.SIM).
- **Performance budget:** app start under 2 s on the minimum hardware profile [unverified until measured].
- **Crate:** `fid-app-probe` (layer `app`).

## Test fixtures

Example `ex-probe-prismatic-sim` from ECO.DATA.EX with its recorded I++ simulator transcript; the open I++ simulator from HAL.SIM.IPPSIM (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, create binary crate `fid-app-probe` (layer `app`). It holds `app.toml` and `src/main.rs` only. `app.toml` (schema v1 of `fid-app-manifest`): `id = "probe"`, `status = "alpha"`, `status_scope = "offline programming and simulation"` (adjust as scope grows), `namespaces = ["project","journal","registry","settings","job","import","export","geo","construct","fit","cmm","device","align","check","gdt","decide","unc","report","sign","view","capture"]`, panels, `layout = "probe.default"`, CMM programming defaults, placeholder branding, known gaps for non-live CMM seed rows. `main.rs` (under 60 lines): load the manifest (exit 2 on findings), install the `fid-app-family` edit guard, start the shell. No domain logic, no command definitions, and no way to alter MCP projection (D-008: M1 never on MCP; agents get only `run:hold` and read-only status). Tests for AC-1..AC-5 of spec node ECO.APPS.PROBE: the manifest check, the `fid mcp --bridge` tool-list equality with no M1 or A0 tools, and the end-to-end replay of `ex-probe-prismatic-sim` against the I++ simulator in the running app versus headless `fid`. Constraints: CONSTITUTION §8; app 1.0 only after P2 (D-017).

## Open questions

- Whether the approval station is a separate binary or a mode of Probe is owned by CMM.EXEC; if it becomes a mode, its panels are added here, but its human-only and motion rules stay in CMM and CORE.BUS.

## Children

_Leaf: none._
