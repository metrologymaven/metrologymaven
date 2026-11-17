---
id: UX.SCREENS.COMMON
title: Shared shell, Shop Setup and first-run screens
level: 3
parent: UX.SCREENS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX.SCREENS-002, REQ-UX.SCREENS-003, REQ-UX.SCREENS-004, REQ-UX.SCREENS-005, REQ-UX.SCREENS-006, REQ-UX.SCREENS-007, REQ-UX.SCREENS-008]
covers: []
depends_on: [UX.SCREENS.INV, UX.COMP, UX.PATTERN]
uses_artifacts: [FLOW.SETUP.JOURNEY, FLOW.PERSONA.MAPS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SCREENS.COMMON: Shared shell, Shop Setup and first-run screens

## Purpose

The screens every app shares and the first-run experience: the main window frame (menu, toolbar, panels, status bar layout), the start screen with recent projects, project open and create, settings, help and about, and the Shop Setup wizard screens around FLOW's setup journey. They come first because every app screen sits inside the shell frame.

## Scope

**In:**
- Wireframes (`WF.*` IDs) and high-fidelity mockups (`MK.*` IDs) for the screens `SCR.SHELL.FRAME`, `SCR.SHELL.START`, `SCR.SHELL.PROJECT`, `SCR.SHELL.SETTINGS`, `SCR.SHELL.HELP`, `SCR.SETUP.WIZARD` (one wireframe per wizard step group), `SCR.SETUP.SAMPLE` [initial list; the inventory in `UX.SCREENS.INV` is authoritative]
- Familiar-workflow notes per screen: which common metrology workflow step it serves, in the project's own words (C9)
- The role layout (every role layout's frame) and the main-task keyboard path per screen
- Design-review records for each wireframe and mockup

**Out** (and which node owns it instead):
- The inventory, ID convention and approval checks (`UX.SCREENS.INV`)
- Components and patterns (`UX.COMP`, `UX.PATTERN`); any new element is added there first
- Journey maps and screen order (FLOW)
- Implementing the screens (VIZ and the app domains)
- Usability tests of the screens (`UX.RESEARCH.USAB`)

## Requirements

- **REQ-UX.SCREENS.COMMON-001** Each screen in scope shall have an approved wireframe registered in the inventory before any implementation leaf that builds it is claimed.
- **REQ-UX.SCREENS.COMMON-002** Each screen in scope shall have an approved mockup in the light, dark and high-contrast themes before the GA release of its app.
- **REQ-UX.SCREENS.COMMON-003** Each wireframe and mockup shall use only package tokens, icons, components and patterns, and shall pass the design review with a result for every trade-dress rule and no incumbent screenshot in any source file.
- **REQ-UX.SCREENS.COMMON-004** Mockups shall use only synthetic or public data at realistic density (where shown: a characteristic table of at least 500 rows and an element tree of at least 10^4 items [unverified density targets]).
- **REQ-UX.SCREENS.COMMON-005** Each screen design shall state its role layout and a keyboard path for its main task; for this module the path shall open a recent project and reach the first command of a job without a pointer.

## Interfaces

- **Offers:** `WF.*` and `MK.*` artefacts and approval records registered in `screens/inventory.json`.
- **Consumes:** `UX.SCREENS.INV` (IDs, checks), `UX.COMP`, `UX.PATTERN`; journey maps from FLOW.SETUP.JOURNEY, FLOW.PERSONA.MAPS (read as artifacts); research findings from `UX.RESEARCH` through the inventory's review records.

## Decomposition hints

FRAME (shell frame, start and project screens), SETUP (Shop Setup and sample job screens), PREFS (settings and help). A natural leaf is "wireframes for one screen group" (one PR, design-review record), followed later by "mockups for that group" before GA.

## Open questions

- Implementation leaves in VIZ and the app domains do not yet cite wireframe IDs; see UX.SCREENS.INV for the citing convention.
- The role layouts (Inspector, Programmer, Reviewer and others) are defined here as frame arrangements and reused by the app children.

## Children

_None yet: status `open`._
