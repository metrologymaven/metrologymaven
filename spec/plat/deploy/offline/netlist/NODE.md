---
id: PLAT.DEPLOY.OFFLINE.NETLIST
title: Network-feature list generated from the registry manifest
level: 4
parent: PLAT.DEPLOY.OFFLINE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
satisfies: [REQ-PLAT.DEPLOY.OFFLINE-005]
covers: []
depends_on: []
uses_artifacts: [CORE.REG.MAN, CORE.STO.SET]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.OFFLINE.NETLIST: Network-feature list

## Purpose

Publishes the list of everything in the product that can use the network, generated rather than hand-written. Because only X1, I1 and D1 commands may open sockets (CONSTITUTION §3.1, CORE.BUS.POL.EGRESS), the list is exactly the registry commands of those classes, each with its consent or policy setting and its default.

## Scope

**In:**
- `docs/deploy/network-features.md` and `network-features.json`, generated from the registry manifest and the settings manifest
- A check that every X1 command names an opt-in setting whose default is off
- A drift check in CI

**Out** (and which node owns it instead):
- Which commands exist and their classes (each owning domain via `CORE.REG`)
- The socket guard (`CORE.BUS.POL.EGRESS`)

## Requirements

- **REQ-PLAT.DEPLOY.OFFLINE.NETLIST-001** The generator shall list every command in the registry manifest whose side-effect class is X1, I1 or D1, with command ID, class, CLI name, owning domain and phase, sorted by command ID.
- **REQ-PLAT.DEPLOY.OFFLINE.NETLIST-002** For each X1 command, the list shall name the consent or enabling setting key declared for it and that key's default, taken from the settings manifest.
- **REQ-PLAT.DEPLOY.OFFLINE.NETLIST-003** If an X1 command declares no enabling setting key, or its key's default is not off (`false`, `off` or `disabled`), then generation shall fail naming the command.
- **REQ-PLAT.DEPLOY.OFFLINE.NETLIST-004** The page shall state that I1 and D1 commands reach only configured simulators and devices and run only on explicit user action, and CI shall fail if the committed page and JSON differ from a fresh generation.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/deploy/manifest-net.json` (commands `update.check@1` X1, `egress.send@1` X1, `device.connect@1` D1, `align.run_bestfit@1` W2) and `fixtures/plat/deploy/settings-manifest.json`, when `cargo xtask plat-deploy netlist` runs, then the output equals `fixtures/plat/deploy/network-features.json` and lists exactly three commands.
- **AC-2** (error path) Given a settings manifest where `update.consent` defaults to `on`, then it exits 1 with `X1DefaultOn { command: "update.check@1" }`.
- **AC-3** (error path) Given an X1 command with no enabling key, then `X1NoOptIn`.
- **AC-4** Given a hand-edited committed page, when `--check` runs, then it fails.

## Design

- **Registry command(s):** none; `xtask plat-deploy netlist [--check]`.
- **Data structures:** reads the manifest JSON from `fid registry show-manifest --full` and `settings-manifest.json`; the enabling key is read from the command definition's policy fields (`opt_in_setting`) [unverified: field name until CORE.REG.DEF.FIELD names it; the fixture fixes it]. For `egress.send@1`, whose approval is per invocation, the enabling key is the destination list, whose default is empty.
- **Errors:** `X1NoOptIn`, `X1DefaultOn`, `ManifestParse`.
- **Determinism:** D0.
- **Crate:** `xtask` (module `plat_deploy_netlist`); reads artifacts only.

## Test fixtures

Fixture manifests under `fixtures/plat/deploy/`. Licensed with the code.

## Build prompt

> Add `xtask plat-deploy netlist [--check]`: read the registry manifest (as exported by `fid registry show-manifest --full`) and `settings-manifest.json`; list every X1, I1 and D1 command (ID, class, CLI name, domain, phase) sorted by ID; for X1 commands add the enabling setting key and its default and fail with `X1NoOptIn` or `X1DefaultOn` unless the default is off/false/disabled (egress destinations: empty list counts as off). Write `network-features.json` (RFC 8785) and `docs/deploy/network-features.md` (generated header, I1/D1 statement). `--check` fails on drift. Tests: AC-1..AC-4. Constraints: CONSTITUTION §8, C7. Commit: `feat(xtask): network-feature list`.

## Open questions

- The registry definition needs a field linking an X1 command to its opt-in setting (cross-domain with `CORE.REG.DEF.FIELD`); until then the fixture names it `opt_in_setting`.

## Children

_Leaf: none._
