---
id: UX.SCREENS.INSPECT
title: Inspect app screens
level: 3
parent: UX.SCREENS
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-UX.SCREENS-002, REQ-UX.SCREENS-003, REQ-UX.SCREENS-004, REQ-UX.SCREENS-005, REQ-UX.SCREENS-006, REQ-UX.SCREENS-007, REQ-UX.SCREENS-008]
covers: []
depends_on: [UX.SCREENS.INV, UX.COMP, UX.PATTERN]
uses_artifacts: [FLOW.INSPECT.J1, FLOW.INSPECT.J7, FLOW.PERSONA.MAPS]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# UX.SCREENS.INSPECT: Inspect app screens

## Purpose

The primary screens of the Inspect app (scan inspection): CAD and mesh import, PMI review, alignment, colour-map deviation view with scale editing, comparison points, sections and thickness, and the inspection program view.

## Scope

**In:**
- Wireframes (`WF.*` IDs) and high-fidelity mockups (`MK.*` IDs) for the screens `SCR.INSP.IMPORT`, `SCR.INSP.PMI`, `SCR.INSP.ALIGN`, `SCR.INSP.CMAP`, `SCR.INSP.POINTS`, `SCR.INSP.SECTION`, `SCR.INSP.PROGRAM` [initial list; the inventory in `UX.SCREENS.INV` is authoritative]
- Familiar-workflow notes per screen: which common metrology workflow step it serves, in the project's own words (C9)
- The role layout (Inspector layout) and the main-task keyboard path per screen
- Design-review records for each wireframe and mockup

**Out** (and which node owns it instead):
- The inventory, ID convention and approval checks (`UX.SCREENS.INV`)
- Components and patterns (`UX.COMP`, `UX.PATTERN`); any new element is added there first
- Journey maps and screen order (FLOW)
- Implementing the screens (VIZ and the app domains)
- Usability tests of the screens (`UX.RESEARCH.USAB`)

## Requirements

- **REQ-UX.SCREENS.INSPECT-001** Each screen in scope shall have an approved wireframe registered in the inventory before any implementation leaf that builds it is claimed.
- **REQ-UX.SCREENS.INSPECT-002** Each screen in scope shall have an approved mockup in the light, dark and high-contrast themes before the GA release of its app.
- **REQ-UX.SCREENS.INSPECT-003** Each wireframe and mockup shall use only package tokens, icons, components and patterns, and shall pass the design review with a result for every trade-dress rule and no incumbent screenshot in any source file.
- **REQ-UX.SCREENS.INSPECT-004** Mockups shall use only synthetic or public data at realistic density (where shown: a characteristic table of at least 500 rows and an element tree of at least 10^4 items [unverified density targets]).
- **REQ-UX.SCREENS.INSPECT-005** Each screen design shall state its role layout and a keyboard path for its main task; for this module the path shall run J1 from import to report with keyboard paths for every step except free-form viewport navigation, which has keyboard camera commands.
- **REQ-UX.SCREENS.INSPECT-006** Wireframes and mockups for `SCR.INSP.CMAP`, `SCR.INSP.PMI`, `SCR.INSP.ALIGN` and `SCR.INSP.POINTS` shall include the spatial interaction patterns specified in `research/viz-ux-sota/UX-INTERACTION-SPEC.md` (histogram-embedded scrubbable colorbar, inline tolerance+uncertainty sparkbars, contextual 3D action strip, 3D tolerance-zone ghosts, cursor-pivot DRF orientation cube, Proposal Inbox wipe curtain and zero-chrome focus mode).

## Interfaces

- **Offers:** `WF.*` and `MK.*` artefacts and approval records registered in `screens/inventory.json`.
- **Consumes:** `UX.SCREENS.INV` (IDs, checks), `UX.COMP`, `UX.PATTERN`; journey maps from FLOW.INSPECT jobs J1 and J7 (read as artifacts); research findings from `UX.RESEARCH` through the inventory's review records.

## Decomposition hints

SETUPWF (import, PMI review, alignment), ANALYZE (colour map, points, sections and thickness), PROGRAM (program view and re-run). A natural leaf is "wireframes for one screen group" (one PR, design-review record), followed later by "mockups for that group" before GA.

## Open questions

- Implementation leaves in VIZ and the app domains do not yet cite wireframe IDs; see UX.SCREENS.INV for the citing convention.

## Children

_None yet: status `open`._
