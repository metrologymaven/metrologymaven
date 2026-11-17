---
id: OUTR.CONTENT.TUT
title: Tutorials per focused app
level: 3
parent: OUTR.CONTENT
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT-001]
covers: []
depends_on: [OUTR.CONTENT.PUB]
uses_artifacts: [ECO.DATA.EX, ECO.APPS.LIST, ECO.APPS.MANIFEST, ECO.REL.PKG]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT: Tutorials per focused app

## Purpose

Step-by-step tutorials, at least one per focused app, each built on a published example project and journal so that a reader can reproduce every step and result. P0 tutorials cover the CLI and journal walkthroughs; app tutorials follow as each app becomes usable.

## Scope

**In:**
- A tutorial list per app with owners and target releases
- P0 tutorials: CLI basics, journals, replay and verification walkthroughs
- App tutorials for Inspect, Probe, Network and Report as their phases make them usable
- The example project and journal behind each tutorial, contributed through ECO's dataset process

**Out** (and which node owns it instead):
- Migration guides (owned by `OUTR.CONTENT.MIG`)
- Videos (owned by `OUTR.CONTENT.VIDEO`)
- Replay checks across releases (owned by `OUTR.CONTENT.REPRO`)
- Manifest and lint checks before publication (owned by `OUTR.CONTENT.PUB`)
- User guide chapters (owned by `OUTR.DOCS.BOOK`)

## Requirements

- **REQ-OUTR.CONTENT.TUT-001** Each focused app declared in the ECO.APPS manifest shall have at least one tutorial once the app's status is alpha or later.
- **REQ-OUTR.CONTENT.TUT-002** Before any app reaches alpha, there shall be at least one P0 tutorial covering running a command from the CLI, reading its journal, replaying it and verifying a result package.
- **REQ-OUTR.CONTENT.TUT-003** Each tutorial shall name its published example project and journal and the release it was last verified against, in the content manifest defined by OUTR.CONTENT.PUB.
- **REQ-OUTR.CONTENT.TUT-004** Each tutorial step that runs a command shall show the command and its expected output, and the expected output shall match the journal replay within the normalisation used by OUTR.CONTENT.REPRO.
- **REQ-OUTR.CONTENT.TUT-005** Each tutorial shall state the time a new user needs to complete it and the prerequisites (release, example project, hardware profile).

## Interfaces

- **Offers:** tutorial pages (registered in OUTR.DOCS.FRAME guide slots); example projects and journals (through ECO.DATA).
- **Consumes:** ECO.DATA example projects; ECO.APPS manifest; OUTR.CONTENT.PUB manifest format and checks.

## Decomposition hints

P0 (CLI and journal tutorials), then one child per app (INSPECT, PROBE, NETWORK, REPORT) as each app reaches alpha.

## Open questions

- CONSTITUTION §2.1 (D-034): the former code dependencies on ECO.DATA and ECO.APPS are now `uses_artifacts` (example projects, `apps.toml`, the app status export, release binaries).
- CLI and journal walkthroughs need no app and could start in P0 (carried from OUTR.CONTENT). Moving this node or its parent to P0 needs a decision (CONSTITUTION §7); it stays P1a here.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.CONTENT.TUT.CHECK](check/NODE.md) | Tutorial coverage and step-output check (leaf) | 001 002 004 005 | none |
| [OUTR.CONTENT.TUT.CORE](core/NODE.md) | First steps: CLI, journal, replay and verification (leaf) | 002 003 004 005 | none |
| [OUTR.CONTENT.TUT.REPORT](report/NODE.md) | Report app tutorial: a transcription inspection report (leaf) | 001 003 004 005 | none |
| [OUTR.CONTENT.TUT.INSPECT](inspect/NODE.md) | Inspect app tutorial: scan-to-CAD inspection (leaf) | 001 003 004 005 | none |
| [OUTR.CONTENT.TUT.PROBE](probe/NODE.md) | Probe app tutorials | 001 003 004 005 | none |
| [OUTR.CONTENT.TUT.NETWORK](network/NODE.md) | Network app tutorials | 001 003 004 005 | none |
