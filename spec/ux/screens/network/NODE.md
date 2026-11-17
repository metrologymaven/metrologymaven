---
id: UX.SCREENS.NETWORK
title: Network app screens
level: 3
parent: UX.SCREENS
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-UX.SCREENS-002, REQ-UX.SCREENS-003, REQ-UX.SCREENS-004, REQ-UX.SCREENS-005, REQ-UX.SCREENS-006, REQ-UX.SCREENS-007, REQ-UX.SCREENS-008]
covers: []
depends_on: [UX.SCREENS.INV, UX.COMP, UX.PATTERN]
uses_artifacts: [FLOW.PERSONA.MAPS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SCREENS.NETWORK: Network app screens

## Purpose

The primary screens of the Network app (large-volume metrology): instrument stations and network view, bundle adjustment results, scale bars, move-and-measure guidance for build and alignment, and network reporting.

## Scope

**In:**
- Wireframes (`WF.*` IDs) and high-fidelity mockups (`MK.*` IDs) for the screens `SCR.NET.STATIONS`, `SCR.NET.ADJUST`, `SCR.NET.SCALE`, `SCR.NET.GUIDE`, `SCR.NET.REPORT` [initial list; the inventory in `UX.SCREENS.INV` is authoritative]
- Familiar-workflow notes per screen: which common metrology workflow step it serves, in the project's own words (C9)
- The role layout (Large-volume metrologist layout) and the main-task keyboard path per screen
- Design-review records for each wireframe and mockup

**Out** (and which node owns it instead):
- The inventory, ID convention and approval checks (`UX.SCREENS.INV`)
- Components and patterns (`UX.COMP`, `UX.PATTERN`); any new element is added there first
- Journey maps and screen order (FLOW)
- Implementing the screens (VIZ and the app domains)
- Usability tests of the screens (`UX.RESEARCH.USAB`)

## Requirements

- **REQ-UX.SCREENS.NETWORK-001** Each screen in scope shall have an approved wireframe registered in the inventory before any implementation leaf that builds it is claimed.
- **REQ-UX.SCREENS.NETWORK-002** Each screen in scope shall have an approved mockup in the light, dark and high-contrast themes before the GA release of its app.
- **REQ-UX.SCREENS.NETWORK-003** Each wireframe and mockup shall use only package tokens, icons, components and patterns, and shall pass the design review with a result for every trade-dress rule and no incumbent screenshot in any source file.
- **REQ-UX.SCREENS.NETWORK-004** Mockups shall use only synthetic or public data at realistic density (where shown: a characteristic table of at least 500 rows and an element tree of at least 10^4 items [unverified density targets]).
- **REQ-UX.SCREENS.NETWORK-005** Each screen design shall state its role layout and a keyboard path for its main task; for this module the path shall run a network adjustment and read its results without a pointer.

## Interfaces

- **Offers:** `WF.*` and `MK.*` artefacts and approval records registered in `screens/inventory.json`.
- **Consumes:** `UX.SCREENS.INV` (IDs, checks), `UX.COMP`, `UX.PATTERN`; journey maps from FLOW.NETWORK acceptance job (read as artifacts); research findings from `UX.RESEARCH` through the inventory's review records.

## Decomposition hints

NETWORK (stations, adjustment, scale), GUIDE (build guidance), NETRPT (network report screen). A natural leaf is "wireframes for one screen group" (one PR, design-review record), followed later by "mockups for that group" before GA.

## Open questions

- Implementation leaves in VIZ and the app domains do not yet cite wireframe IDs; see UX.SCREENS.INV for the citing convention.
- FLOW.NETWORK is still `open`; its job map should be added to `uses_artifacts` when it exists.

## Children

_None yet: status `open`._
