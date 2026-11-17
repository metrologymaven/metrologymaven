---
id: ECO.APPS
title: App family: focused apps composed from shared crates
level: 2
parent: ECO
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-ECO-001, REQ-ECO-010]
covers: []
depends_on: [ECO.REPO, CORE, VIZ, AUTO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.APPS: App family

## Purpose

Defines which focused desktop and web apps the project ships and how each one is put together from shared crates: the app manifest (which command namespaces, panels, defaults and report templates an app enables), the rule that apps hold policy and libraries do not, cross-app project hand-off, and each app's declared status. It exists as a separate unit because the app list and its composition rules are a maintainer decision (D-004) that every other ECO child builds on.

## Scope

**In:**
- The list of focused apps and their one-line jobs (proposed in D-004: scan inspection, CMM programming, large-volume networks, reporting and SPC)
- The app manifest: enabled command namespaces, UI panels, default settings and workspace layout, report templates, branding slot
- The "no app policy in shared crates" rule and how it is reviewed
- Cross-app opening of the same project, and launching one app from another on the same project
- Per-app status (alpha, beta, stable) and known-gaps declaration, as data consumed by ECO.PARITY
- A future "suite" host that runs several apps in one window (D-004 option C), as a hook only

**Out** (and which node owns it instead):
- Crate graph, repo layout and layering checks (owned by `ECO.REPO`)
- Command definitions and the project model (owned by `CORE` and each domain)
- The shared viewer and UI widgets (owned by `VIZ`)
- Installers and release channels for each app (owned by `ECO.REL`)
- Per-app website pages (owned by `OUTR`)
- OS and hardware support per app (owned by `PLAT`)

## Requirements

- **REQ-ECO.APPS-001** The project shall publish an app list in which each app has a stable identifier, a one-line job, an owning maintainer and a phase.
- **REQ-ECO.APPS-002** Each app shall be defined by a declarative manifest that selects registry command namespaces, panels, defaults and templates, and an app crate shall contain no measurement, fitting, evaluation or file-format logic of its own.
- **REQ-ECO.APPS-003** When a command is reachable in one app, it shall have the same ID, parameters and result in every other app that enables it.
- **REQ-ECO.APPS-004** Every app shall open, save and journal the same project container, so that a project saved by one app opens in another without conversion or loss of journal entries.
- **REQ-ECO.APPS-005** If a project contains elements whose commands the opening app does not enable, then the app shall show those elements read-only with their provenance and shall offer to open the project in an app that enables them.
- **REQ-ECO.APPS-006** Each app manifest shall declare the app's status and its known gaps as machine-readable data, which ECO.PARITY reads to generate status pages.
- **REQ-ECO.APPS-007** Adding or removing an app shall require an RFC with the longer comment period defined by ECO.COMM.

## Interfaces

- **Offers:** the app list; the app manifest schema; per-app status and known-gaps data; the cross-app hand-off contract.
- **Consumes:** CORE registry manifest and project container; VIZ shell and widgets; AUTO generated surfaces (the `fid` CLI and MCP servers stay shared across apps, so agents see one platform).

## Decomposition hints

LIST (app list and ownership), MANIFEST (schema and loader), HANDOFF (cross-app open and launch), STATUS (status and gaps data), SUITE (later, option C host).

## Open questions

- Blocked on D-004 (one integrated app vs a family of focused apps). The split of this node depends on the chosen option; the requirements above are written for option B and still hold, with a single entry in the app list, under option A.
- D-005 (UI toolkit and TypeScript boundary) affects how panels are declared in the manifest.

## Children

_None yet: status `decision`._
