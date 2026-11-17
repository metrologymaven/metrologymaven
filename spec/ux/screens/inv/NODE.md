---
id: UX.SCREENS.INV
title: Screen inventory, wireframe-ID convention and approval records
level: 3
parent: UX.SCREENS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.SCREENS-001, REQ-UX.SCREENS-002, REQ-UX.SCREENS-003, REQ-UX.SCREENS-008, REQ-UX.SCREENS-009]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.SYSTEM.REVIEW]
uses_artifacts: [FLOW.PERSONA.SCHEMA, FLOW.PERSONA.MAPS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SCREENS.INV: Screen inventory, wireframe-ID convention and approval records

## Purpose

The register that makes REQ-UX-003 enforceable across the tree: the list of every primary screen with a stable screen ID that FLOW journey maps use in their `screen` field, the ID convention for wireframes and mockups that implementation leaves cite, the approval records, and a check that reports untraced screens, missing approvals, stale approvals after a journey change, and implementation leaves that cite an unapproved wireframe.

## Scope

**In:**
- `screens/inventory.json` (`screen-inventory@1`)
- The ID convention (below) and `screens/README.md` explaining how leaves cite wireframes
- Approval records: `design-review@1` records extended with `screen`, `artefact` (`WF` or `MK` ID) and `journey_digests`
- `fid-design screens-check` (inventory, trace, approvals, staleness) and `fid-design screens-gate` (spec-tree citations)

**Out:** the wireframes and mockups themselves (the per-app SCREENS children); journey maps (FLOW); the review gate itself (`UX.SYSTEM.REVIEW`); adding a frontmatter field or a scope.py rule to the spec tooling (maintainers' tooling PR).

## ID convention

- **Screen ID:** `SCR.<APP>.<NAME>`, with `APP` in `SHELL`, `SETUP`, `INSP`, `PROBE`, `NET`, `RPT`, `KIOSK`, `STN` and `NAME` 2 to 12 uppercase letters or digits starting with a letter, e.g. `SCR.RPT.FAIFORM`. This is the value FLOW journey maps put in a step's `screen` field.
- **Wireframe ID:** `WF.<APP>.<NAME>@<rev>`; **mockup ID:** `MK.<APP>.<NAME>@<rev>`; `rev` is a positive integer that increases with each approved revision, e.g. `WF.RPT.FAIFORM@2`.
- **Files:** `screens/<app>/<name>/wf-<rev>.{svg,pdf,png}` and `mk-<rev>.<theme>.{png,pdf}`.
- **Citing:** an implementation leaf that builds a primary screen lists the IDs it builds against. Until maintainers add a frontmatter field (proposed `wireframes: [WF.RPT.FAIFORM@2]`, list items without commas), the leaf writes a line `Wireframes: WF.RPT.FAIFORM@2` in its `## Design` section; `screens-gate` reads both forms.

## Requirements

- **REQ-UX.SCREENS.INV-001** The inventory shall list every primary screen of Inspect, Probe, Network and Report, the kiosk, the Approval Station, Shop Setup and the shared shell, each with screen ID, app, title, role layout, phase, and at least one journey trace (journey map ID and step number).
- **REQ-UX.SCREENS.INV-002** Each inventory entry shall hold the keyboard path of its main task and the IDs of its current approved wireframe and mockup, or `none`.
- **REQ-UX.SCREENS.INV-003** If an inventory entry has no journey trace, or a trace names a map or step absent from the given journey-map directory, then `screens-check` shall fail and name the screen.
- **REQ-UX.SCREENS.INV-004** An approval record shall store the RFC 8785 SHA-256 digest of each journey map the screen traces to; when a current map's digest differs, `screens-check` shall report the approval as `stale` until a new record is added.
- **REQ-UX.SCREENS.INV-005** When `fid-design screens-gate` runs over the spec tree, it shall report every leaf that cites a WF or MK ID that has no approval record, and every leaf whose status is claimed (`claimed_by` set) and whose `## Design` names a screen ID without citing a wireframe of it.
- **REQ-UX.SCREENS.INV-006** With `--ga <app>`, `screens-check` shall report every primary screen of that app without an approved mockup.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/screens/inv/ok/` (inventory of 4 screens, 2 journey maps, approvals with matching digests), when `fid-design screens-check --root ok/design --journeys ok/journeys --json` runs, then it exits 0.
- **AC-2** (error path) Given `untraced/`, when the check runs, then it exits 1 with `SCREEN-UNTRACED SCR.RPT.FAIFORM`.
- **AC-3** Given `stale/` (a journey map changed after approval), when the check runs, then it exits 1 with `SCREEN-APPROVAL-STALE SCR.RPT.FAIFORM WF.RPT.FAIFORM@1`.
- **AC-4** (error path) Given spec-tree fixture `fixtures/ux/screens/inv/tree/` where a leaf's Design cites `WF.RPT.FAIFORM@3` and only `@2` is approved, when `fid-design screens-gate --spec tree --root ok/design --json` runs, then it exits 1 with `SCREEN-GATE-UNAPPROVED`.
- **AC-5** Given `ok/` with `--ga report` and one Report screen lacking a mockup, then it exits 1 with `SCREEN-GA-MOCKUP-MISSING`.
- **AC-6** (error path) Given `bad-id/` (`SCR.rpt.form`), when the check runs, then it exits 1 with `SCREEN-ID-SYNTAX`.

## Design

- **Commands:** `fid-design screens-check --root <design dir> --journeys <dir> [--ga <app>] [--json]`; `fid-design screens-gate --spec <spec dir> --root <design dir> [--json]`. Tooling, nearest class R0.
- **Inventory record:** `{ id, app, title, role_layout, phase, traces: [{ map, step }], keyboard_path, wireframe, mockup }`.
- **Digests:** journey maps canonicalised by RFC 8785 then SHA-256, matching FLOW.PERSONA.SCHEMA's digest.
- **Errors:** `SCREEN-SCHEMA`, `SCREEN-ID-SYNTAX`, `SCREEN-UNTRACED`, `SCREEN-TRACE-UNKNOWN`, `SCREEN-APPROVAL-MISSING`, `SCREEN-APPROVAL-STALE`, `SCREEN-GATE-UNAPPROVED`, `SCREEN-GATE-UNCITED`, `SCREEN-GA-MOCKUP-MISSING`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `screens`.

## Test fixtures

`fixtures/ux/screens/inv/`: `ok/`, `untraced/`, `stale/`, `bad-id/`, `tree/` (a tiny spec tree with two leaves); synthetic, CC-BY-4.0.

## Build prompt

> Create `screens/inventory.json` (`screen-inventory@1`: id `SCR.<APP>.<NAME>` with APP in SHELL|SETUP|INSP|PROBE|NET|RPT|KIOSK|STN, app, title, role_layout, phase, traces [{map, step}], keyboard_path, wireframe `WF.<APP>.<NAME>@<rev>` or none, mockup `MK.<APP>.<NAME>@<rev>` or none) and `screens/README.md` (ID convention; implementation leaves cite wireframes in a `wireframes:` frontmatter list once added, otherwise a `Wireframes:` line in `## Design`). In the Rust tooling crate `fid-design`, add `fid-design screens-check --root <dir> --journeys <dir> [--ga <app>] [--json]` (ID syntax, traces resolve to journey-map steps, approvals present, staleness by comparing stored RFC 8785 SHA-256 journey digests with current ones, GA mockup completeness) and `fid-design screens-gate --spec <dir> --root <dir> [--json]` (scan NODE.md files for cited WF/MK IDs in frontmatter or Design, report unapproved citations and claimed leaves naming a screen without a wireframe citation). Errors as in spec node UX.SCREENS.INV. Exit 0, 1, 2. Tests AC-1..AC-6.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; RFC 8785 via the project canonical-JSON crate; no network.

## Open questions

- Enforcing the gate in the spec tree needs a maintainers' tooling change: a `wireframes:` frontmatter field and a scope.py or CI call to `screens-gate`. Until then the gate is advisory.
- FLOW.PERSONA.SCHEMA notes screen IDs are free strings until UX publishes an inventory; it can now validate them against `screens/inventory.json` (`uses_artifacts: [UX.SCREENS.INV]` on the FLOW side, not editable from here).

## Children

_Leaf: none._
