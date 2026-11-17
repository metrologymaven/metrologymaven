---
id: ECO.APPS.HANDOFF.EDITORS
title: Which installed apps can edit an element
level: 4
parent: ECO.APPS.HANDOFF
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.HANDOFF-003]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, CORE.PRJ.TREE, CORE.REG.QRY]
uses_artifacts: [ECO.REL.PKG.REG]
side_effect: R0
determinism: D0
cli: [fid app list-editors]
mcp: [app_list_editors]
journal: [app.list_editors@1]
updated: 2026-11-17
---

# ECO.APPS.HANDOFF.EDITORS: Which installed apps can edit an element

## Purpose

Answers one question for people, agents and the "Open in" menu: which installed apps of the family enable the command namespace that created a given element, so the element can be edited there. It reads the installed-app registration entries that packaging writes, loads each installed app's manifest through the generic manifest loader, and returns a sorted list. It is the only registry command of the hand-off module.

## Scope

**In:**
- Registry command `app.list_editors@1` (R0) and its generated CLI and MCP surfaces
- Resolving an element's creating namespace (the part of its creating command ID before the first dot)
- Reading installed-app registration entries and each app's manifest, offline

**Out:**
- Writing the registration entries at install time (owned by `ECO.REL.PKG`)
- Refusing edits and showing provenance (owned by `ECO.APPS.HANDOFF.RO`)
- Launching the chosen app (owned by `ECO.APPS.HANDOFF.OPENIN`)
- The manifest format and loader (owned by `ECO.APPS.MANIFEST.SCHEMA`)

## Requirements

- **REQ-ECO.APPS.HANDOFF.EDITORS-001** When `app.list_editors@1` runs for an element ID, the command shall return every installed app whose manifest enables the namespace of the element's creating command, as `{app_id, version, install_ref}` entries sorted by `app_id` then `version`.
- **REQ-ECO.APPS.HANDOFF.EDITORS-002** When `app.list_editors@1` runs with a namespace instead of an element ID, the command shall return the same list for that namespace.
- **REQ-ECO.APPS.HANDOFF.EDITORS-003** If an installed app's registration entry or manifest cannot be read or fails validation, then the command shall leave that app out of the list and add a warning naming the app and the reason, and shall not fail.
- **REQ-ECO.APPS.HANDOFF.EDITORS-004** The command shall make no network access and shall return the full path of no file; `install_ref` is an opaque handle that only ECO.APPS.HANDOFF.OPENIN resolves.
- **REQ-ECO.APPS.HANDOFF.EDITORS-005** If the element ID is unknown in the open project, then the command shall fail with `APP-ELEMENT-UNKNOWN`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/handoff/installs/` (registration entries for `report` 0.3.0 and `inspect` 0.2.0, whose manifests enable `report` and `compare` respectively) and project `fixtures/handoff/mixed.fidproj` with element `E-cmp` created by `compare.run_surface@1`, when `fid app list-editors --element E-cmp --json` runs, then the result lists exactly `inspect` 0.2.0.
- **AC-2** Given the same installs, when `fid app list-editors --namespace report --json` runs, then the result lists exactly `report` 0.3.0.
- **AC-3** (degraded) Given `fixtures/handoff/installs-broken/` where the `inspect` manifest names an unknown panel, when AC-1 runs, then the list is empty and `warnings` contains `inspect: MAN-UNKNOWN-PANEL`; the exit code is 0.
- **AC-4** (error path) Given an element ID absent from the project, when the command runs, then it fails with `APP-ELEMENT-UNKNOWN` and nothing is written to the Record Journal.
- **AC-5** (privacy) The JSON result of AC-1 contains no absolute path (checked by a regex for drive letters and leading `/`).
- **AC-6** (determinism) Two runs of AC-1 with the registration entries enumerated in reverse order give byte-identical JSON.

## Design

- **Registry command(s):** `app.list_editors@1`, R0, MCP exposed; params `{ element?: ElementId, namespace?: Namespace }` (exactly one); result `{ editors: [{ app_id, version, install_ref }], warnings: [String] }`. Not written to the Record Journal; it goes to the Activity Log only under the D-031 read rule.
- **Data structures:** `InstallRef` (opaque, index into the registration list read at call time); registration entry fields read: `app_id`, `version`, executable path (format owned by ECO.REL.PKG.REG); the app's `app.toml` is read from beside the executable, where the installer places it.
- **Algorithm:** resolve namespace (element → creating command ID from the element tree → prefix); enumerate registration entries; load each manifest with the generic loader in validate-only mode against the current registry manifest; keep apps whose effective namespaces include it; sort.
- **Errors:** `APP-ELEMENT-UNKNOWN`, `APP-ARGS` (both or neither parameter).
- **Determinism:** D0; sorted output, no clock.
- **Performance budget:** under 100 ms with 10 installed apps [unverified until measured].
- **Crate:** `fid-app-family` (shared app machinery, assigned in `layers.toml` below the `app` layer so it counts as a shared crate for ECO.APPS.POLICY).

## Test fixtures

Hand-written registration entries, manifests and a small project under `fixtures/handoff/` (MIT OR Apache-2.0).

## Build prompt

> In the implementation repository, create crate `fid-app-family` (shared app machinery; put it in `layers.toml` in the shell layer below `app`) with module `editors` implementing registry command `app.list_editors@1` (R0, MCP exposed, CLI `fid app list-editors`, MCP `app_list_editors`). Params: exactly one of `element: ElementId` or `namespace: String`; result `{editors: [{app_id, version, install_ref}], warnings: [String]}` sorted by app_id then version. Resolve an element's namespace from its creating command ID (prefix before the first dot) via the project element tree. Enumerate installed-app registration entries (format from ECO.REL.PKG.REG: app ID, version, executable path, scope; read offline), load each app's `app.toml` from beside its executable with `fid-app-manifest`'s loader in validate-only mode, and keep apps whose effective namespaces include the namespace. A broken install is skipped with a warning, never an error. `install_ref` is opaque; never return absolute paths. Errors: `APP-ELEMENT-UNKNOWN`, `APP-ARGS`. Write fixtures under `fixtures/handoff/` and tests for AC-1..AC-6 of spec node ECO.APPS.HANDOFF.EDITORS. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe` in non-test code, bounded parsing, fail closed); no network; BTreeMap for ordered output; reads are not journaled (D-031).

## Open questions

- Cross-domain (ECO.REL.PKG.REG): its entries record app ID, version, executable path and scope but not where the app manifest is. This leaf assumes installers place `app.toml` beside the executable; ECO.REL.PKG.REG should state that (or add a manifest path field).

## Children

_Leaf: none._
