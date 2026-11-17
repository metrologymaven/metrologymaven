---
id: UX.SCREENS.RPTAPP
title: Report app screens
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
uses_artifacts: [FLOW.REPORT.J2, FLOW.REPORT.J3, FLOW.REPORT.J6, FLOW.REPORT.J8, FLOW.REPORT.KBD, FLOW.PERSONA.MAPS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SCREENS.RPTAPP: Report app screens

## Purpose

The primary screens of the Report app (reporting and SPC; the earliest app to ship): characteristic table workspace, ballooning on a drawing sheet, transcription from an existing report, FAI form view, supplier package intake and verification result, SPC dashboard and chart views, and report preview and release. These are application screens; report page designs belong to UX.REPORT.

## Scope

**In:**
- Wireframes (`WF.*` IDs) and high-fidelity mockups (`MK.*` IDs) for the screens `SCR.RPT.CHARTAB`, `SCR.RPT.BALLOON`, `SCR.RPT.TRANSCR`, `SCR.RPT.FAIFORM`, `SCR.RPT.INTAKE`, `SCR.RPT.SPCDASH`, `SCR.RPT.PREVIEW` [initial list; the inventory in `UX.SCREENS.INV` is authoritative]
- Familiar-workflow notes per screen: which common metrology workflow step it serves, in the project's own words (C9)
- The role layout (Reviewer and quality-engineer layouts) and the main-task keyboard path per screen
- Design-review records for each wireframe and mockup

**Out** (and which node owns it instead):
- The inventory, ID convention and approval checks (`UX.SCREENS.INV`)
- Components and patterns (`UX.COMP`, `UX.PATTERN`); any new element is added there first
- Journey maps and screen order (FLOW)
- Implementing the screens (VIZ and the app domains)
- Usability tests of the screens (`UX.RESEARCH.USAB`)

## Requirements

- **REQ-UX.SCREENS.RPTAPP-001** Each screen in scope shall have an approved wireframe registered in the inventory before any implementation leaf that builds it is claimed.
- **REQ-UX.SCREENS.RPTAPP-002** Each screen in scope shall have an approved mockup in the light, dark and high-contrast themes before the GA release of its app.
- **REQ-UX.SCREENS.RPTAPP-003** Each wireframe and mockup shall use only package tokens, icons, components and patterns, and shall pass the design review with a result for every trade-dress rule and no incumbent screenshot in any source file.
- **REQ-UX.SCREENS.RPTAPP-004** Mockups shall use only synthetic or public data at realistic density (where shown: a characteristic table of at least 500 rows and an element tree of at least 10^4 items [unverified density targets]).
- **REQ-UX.SCREENS.RPTAPP-005** Each screen design shall state its role layout and a keyboard path for its main task; for this module the path shall complete J2 and J3 keyboard-only, matching FLOW.REPORT.KBD.

## Interfaces

- **Offers:** `WF.*` and `MK.*` artefacts and approval records registered in `screens/inventory.json`.
- **Consumes:** `UX.SCREENS.INV` (IDs, checks), `UX.COMP`, `UX.PATTERN`; journey maps from FLOW.REPORT jobs J2, J3, J6, J8 and FLOW.REPORT.KBD (read as artifacts); research findings from `UX.RESEARCH` through the inventory's review records.

## Decomposition hints

FAI (characteristic table, ballooning, transcription, FAI form), INTAKE (supplier package intake and verification), SPC (SPC dashboard and charts), RELEASE (preview and release). A natural leaf is "wireframes for one screen group" (one PR, design-review record), followed later by "mockups for that group" before GA.

## Open questions

- Implementation leaves in VIZ and the app domains do not yet cite wireframe IDs; see UX.SCREENS.INV for the citing convention.

## Children

_None yet: status `open`._
