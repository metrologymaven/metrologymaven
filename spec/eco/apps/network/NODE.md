---
id: ECO.APPS.NETWORK
title: Network app composition (large-volume networks)
level: 3
parent: ECO.APPS
kind: task
status: leaf
phase: P3
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, ECO.APPS.HANDOFF.RO, VIZ.SHELL.DOCK, LVM.STATION, LVM.ADJ, LVM.SCALE, LVM.MOVE, LVM.RPT, UNC.PROP, ALN.XFM, GDT.CHAR, RPT.MODEL, TRUST.PKG]
uses_artifacts: [ECO.DATA.EX, AUTO.CLI.TARGET.APP, CORE.RPL.EXEC, ECO.PARITY.STATUS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.NETWORK: Network app composition

## Purpose

Defines the Network app (D-004): the focused app for portable and large-volume instrument networks (stations, bundle adjustment, transformations, live guidance, uncertainty). This leaf owns its manifest content, defaults and end-to-end acceptance test, not the network logic: the app crate is a manifest plus a start-up call into shared crates.

## Scope

**In:**
- Network app manifest (`app.toml`): enabled namespaces (project, instrument stations, network adjustment, transformations, live guidance, uncertainty, evaluation, reporting), panels, layout, defaults, status and known gaps
- Default workspace layout and defaults for large-volume work
- The app binary entry point
- End-to-end acceptance workflow on a recorded network dataset with no live device attached

**Out** (and which node owns it instead):
- Network adjustment and guidance (owned by `LVM`), instruments (owned by `HAL`), uncertainty (owned by `UNC`)
- Manifest schema (owned by `ECO.APPS.MANIFEST.SCHEMA`)
- Other apps (owned by `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`)

## Requirements

- **REQ-ECO.APPS.NETWORK-001** The Network manifest shall enable the namespaces `project`, `journal`, `registry`, `settings`, `job`, `import`, `export`, `geo`, `construct`, `fit`, `net`, `build`, `device`, `align`, `unc`, `check`, `gdt`, `decide`, `report`, `sign`, `view` and `capture`, which cover building, adjusting and reporting a multi-station network from recorded instrument data with no live device attached.
- **REQ-ECO.APPS.NETWORK-002** When the reference network example is replayed in the running Network app, its adjusted coordinates and stated uncertainties (as ResultRecord digests) shall equal those from replaying the same journal headless with the `fid` CLI.
- **REQ-ECO.APPS.NETWORK-003** When the release status report is generated, it shall show no consistency finding for app `network`.
- **REQ-ECO.APPS.NETWORK-004** The Network manifest shall declare status `alpha` or `beta` with a `status_scope` until every P3 node holding the large-volume workflow is finished; an offline-adjustment subset released earlier shall be labelled by that scope (D-017).
- **REQ-ECO.APPS.NETWORK-005** The Network defaults shall use millimetres for coordinates, 0.001 mm display resolution and a coverage factor k = 2 for displayed expanded uncertainties [unverified as typical large-volume defaults; the app owner may change them in a PR].

## Acceptance criteria

- **AC-1** Given the Network `app.toml` and the release catalogues, when `fid-app-manifest check --manifest crates/fid-app-network/app.toml --catalogues <release catalogues>` runs, then it exits 0.
- **AC-2** (end to end) Given example `ex-network-recorded` (recorded multi-station observations, no device), when the test runs `fid --app network journal replay-exact <journal> --json` in the running app and the same replay headless with `fid`, then both outcomes are `IDENTICAL` and the adjusted-coordinate and uncertainty ResultRecord digests are equal.
- **AC-3** (offline) During AC-2, the device runtime reports no connected device and the journals contain no D1 or M1 command.
- **AC-4** Given the release `status.json`, then the `network` entry has zero consistency findings and a non-empty `status_scope` while not `stable`.
- **AC-5** A source listing of `crates/fid-app-network/src/` contains only `main.rs`, under 60 lines.

## Design

- **Registry command(s):** none; the app enables commands and defines none. `side_effect: R0` records that the leaf adds no command.
- **Data structures:** `crates/fid-app-network/app.toml`: `id = "network"`, `status = "alpha"`, `status_scope = "offline network adjustment from recorded data"`, namespaces as REQ-001, panels (station list, observation table, network viewport, adjustment residuals, uncertainty field legend), `layout = "network.default"`, defaults as REQ-005, placeholder branding, known gaps for large-volume seed rows not yet live (live guidance and watch windows until LVM.GUIDE and LVM.WATCH are live).
- **Algorithm:** identical entry point to the other apps: load manifest → refuse on findings (exit 2) → install the shared edit guard → start the shell.
- **Errors:** start-up refusal with the loader's findings.
- **Determinism:** D0 for computed values; adjustment determinism classes are owned by LVM.
- **Performance budget:** app start under 2 s on the minimum hardware profile [unverified until measured].
- **Crate:** `fid-app-network` (layer `app`).

## Test fixtures

Example `ex-network-recorded` from ECO.DATA.EX (recorded or simulated observations from HAL.SIM.PORT, licence recorded in the catalogue).

## Build prompt

> In the implementation repository, create binary crate `fid-app-network` (layer `app`). It holds `app.toml` and `src/main.rs` only. `app.toml` (schema v1 of `fid-app-manifest`): `id = "network"`, `status = "alpha"`, `status_scope = "offline network adjustment from recorded data"`, `namespaces = ["project","journal","registry","settings","job","import","export","geo","construct","fit","net","build","device","align","unc","check","gdt","decide","report","sign","view","capture"]`, panels, `layout = "network.default"`, defaults (mm, 0.001 mm display, k = 2), placeholder branding, known gaps for non-live large-volume seed rows. `main.rs` (under 60 lines): load the manifest (exit 2 on findings), install the `fid-app-family` edit guard, start the shell. No domain logic and no command definitions. Tests for AC-1..AC-5 of spec node ECO.APPS.NETWORK, including the end-to-end replay of `ex-network-recorded` in the running app versus headless `fid`, with no device connected. Constraints: CONSTITUTION §8; app 1.0 only after P3 (D-017).

## Open questions

- D-006 notes a possible later decision to move LVM offline adjustment to P1b. If that happens, an offline-only Network beta could start in P1b; this node's phase would move with it by decision.
- Whether live tracker motion commands (if any are M1) are enabled here is decided by HAL.LIVE and LVM.GUIDE; the manifest does not forbid M1 so the approval rules of D-008 apply unchanged.

## Children

_Leaf: none._
