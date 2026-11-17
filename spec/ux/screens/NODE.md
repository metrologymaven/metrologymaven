---
id: UX.SCREENS
title: Wireframes and mockups of primary screens
level: 2
parent: UX
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-UX-003, REQ-UX-006]
covers: []
depends_on: [UX.COMP, UX.PATTERN, UX.RESEARCH, FLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX.SCREENS: Wireframes and mockups of primary screens

## Purpose

The screen inventory and the designs for every primary screen of Inspect, Probe, Network and Report, plus the kiosk, the Approval Station and Shop Setup: wireframes approved before implementation and high-fidelity mockups reviewed before GA. It is separate because it applies the system to concrete screens, traced to FLOW's journeys, and because REQ-UX-003 gates implementation leaves across VIZ and the app domains.

## Scope

**In:**
- Screen inventory traced to FLOW journey steps
- Wireframes and high-fidelity mockups per primary screen, per app
- Familiar-workflow mapping: which common metrology workflow step each screen serves, described in our own words (C9)
- Role layouts (for example Inspector, Programmer, Reviewer) as designed screens
- Design approval records and their links to implementation leaves
- Synthetic or public sample data for mockups

**Out** (and which node owns it instead):
- Components and patterns used on the screens (`UX.COMP`, `UX.PATTERN`)
- Journey maps and screen order (`FLOW`)
- Report page designs (`UX.REPORT`)
- Implementing the screens (`VIZ` and the app domains)
- Usability testing of the screens (`UX.RESEARCH`)

## Requirements

- **REQ-UX.SCREENS-001** The screen inventory shall list every primary screen of each app, the kiosk, the Approval Station and Shop Setup, and shall trace each to at least one FLOW journey step.
- **REQ-UX.SCREENS-002** Each primary screen shall have a wireframe with a recorded design-review approval before any implementation leaf that builds it is claimed, and that leaf shall reference the wireframe's ID.
- **REQ-UX.SCREENS-003** Each primary screen shall have a high-fidelity mockup with a recorded design review before the GA release of its app.
- **REQ-UX.SCREENS-004** Mockups shall use only design-system tokens, icons, components and patterns, and any new element shall be added to the design system before the mockup is approved.
- **REQ-UX.SCREENS-005** Each mockup shall exist in the light, dark and high-contrast themes, and kiosk and Approval Station mockups also in the shop-floor theme.
- **REQ-UX.SCREENS-006** Each primary-screen design review shall record the trade-dress distinctness check, and no published design file shall contain an incumbent screenshot.
- **REQ-UX.SCREENS-007** Mockups shall use only synthetic or public data at realistic density, including a characteristic table of at least 500 rows and an element tree of at least 10⁴ items where the screen shows them [unverified density targets].
- **REQ-UX.SCREENS-008** Each primary screen design shall state the keyboard path for its main task and the role layout it belongs to.
- **REQ-UX.SCREENS-009** When FLOW changes a journey step that a screen traces to, the screen's approval shall be marked stale until re-reviewed.

## Interfaces

- **Offers:** the screen inventory, wireframe and mockup IDs, approval records that implementation leaves cite.
- **Consumes:** `UX.COMP`, `UX.PATTERN`, `UX.RESEARCH` (findings that shape layouts), `FLOW` (journeys and screens per step). Interface notes (no `depends_on`): `VIZ.SHELL`, `VIZ.KIOSK`, `VIZ.INBOX`, `CMM.EXEC.STATION` and app-domain UI leaves should cite wireframe IDs from here; the gating is checked by process in this node, not by a dependency edge.

## Decomposition hints

INV (screen inventory and approval records), INSPECT, PROBE, NETWORK, RPTAPP (one per app; RPTAPP is the Report app's screens, not report pages, which `UX.REPORT` owns), FLOOR (kiosk and Approval Station), SETUP (Shop Setup and first run).

## Open questions

- REQ-UX.SCREENS-002 gates implementation leaves in other domains. Those leaves do not yet cite wireframe IDs; adding a `wireframe:` frontmatter field or a scope.py check is a tooling change for maintainers.
- The app list follows D-004; any change to the app family changes the inventory.

## Children

_None yet: status `open`._
