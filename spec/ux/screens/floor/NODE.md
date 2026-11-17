---
id: UX.SCREENS.FLOOR
title: Kiosk and Approval Station screens
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
uses_artifacts: [FLOW.INSPECT.J5, FLOW.PERSONA.MAPS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SCREENS.FLOOR: Kiosk and Approval Station screens

## Purpose

The shop-floor screens: the kiosk (operator picks a released program, follows step prompts, sees the cycle result) and the Approval Station (a human releases and approves runs and motion, separately from requesting them). They use the floor theme as well as the other three themes, large targets and the operator-prompt and confirmation patterns.

## Scope

**In:**
- Wireframes (`WF.*` IDs) and high-fidelity mockups (`MK.*` IDs) for the screens `SCR.KIOSK.LOGIN`, `SCR.KIOSK.PICK`, `SCR.KIOSK.STEP`, `SCR.KIOSK.RESULT`, `SCR.STN.QUEUE`, `SCR.STN.REVIEW`, `SCR.STN.APPROVE` [initial list; the inventory in `UX.SCREENS.INV` is authoritative]
- Familiar-workflow notes per screen: which common metrology workflow step it serves, in the project's own words (C9)
- The role layout (Operator and Approver layouts) and the main-task keyboard path per screen
- Design-review records for each wireframe and mockup

**Out** (and which node owns it instead):
- The inventory, ID convention and approval checks (`UX.SCREENS.INV`)
- Components and patterns (`UX.COMP`, `UX.PATTERN`); any new element is added there first
- Journey maps and screen order (FLOW)
- Implementing the screens (VIZ and the app domains)
- Usability tests of the screens (`UX.RESEARCH.USAB`)

## Requirements

- **REQ-UX.SCREENS.FLOOR-001** Each screen in scope shall have an approved wireframe registered in the inventory before any implementation leaf that builds it is claimed.
- **REQ-UX.SCREENS.FLOOR-002** Each screen in scope shall have an approved mockup in the light, dark and high-contrast themes and the floor theme before the GA release of its app.
- **REQ-UX.SCREENS.FLOOR-003** Each wireframe and mockup shall use only package tokens, icons, components and patterns, and shall pass the design review with a result for every trade-dress rule and no incumbent screenshot in any source file.
- **REQ-UX.SCREENS.FLOOR-004** Mockups shall use only synthetic or public data at realistic density (where shown: a characteristic table of at least 500 rows and an element tree of at least 10^4 items [unverified density targets]).
- **REQ-UX.SCREENS.FLOOR-005** Each screen design shall state its role layout and a keyboard path for its main task; for this module the path shall complete a kiosk cycle with a barcode scanner and physical keys only, and approve a run without a pointer.

## Interfaces

- **Offers:** `WF.*` and `MK.*` artefacts and approval records registered in `screens/inventory.json`.
- **Consumes:** `UX.SCREENS.INV` (IDs, checks), `UX.COMP`, `UX.PATTERN`; journey maps from FLOW.INSPECT job J5 and the Probe approval journey (read as artifacts); research findings from `UX.RESEARCH` through the inventory's review records.

## Decomposition hints

KIOSK (kiosk screens, P1b), STATION (Approval Station screens, P2). A natural leaf is "wireframes for one screen group" (one PR, design-review record), followed later by "mockups for that group" before GA.

## Open questions

- Implementation leaves in VIZ and the app domains do not yet cite wireframe IDs; see UX.SCREENS.INV for the citing convention.
- The Approval Station is needed from P2 (live devices); its wireframes may be a later child with phase P2.
- REQ-UX.SCREENS-005 requires these mockups also in the floor theme.

## Children

_None yet: status `open`._
