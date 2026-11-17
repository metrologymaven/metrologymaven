---
id: UX.SCREENS
title: Wireframes and mockups of primary screens
level: 2
parent: UX
kind: subsystem
status: split
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
updated: 2026-11-17
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

## Split rationale

The dividing principle is **one register plus one child per app surface**, following the decomposition hints: INV (inventory, the wireframe-ID convention, approval records, staleness and the implementation gate) is a leaf because it is one format and one check; COMMON (shared shell, Shop Setup and first run), RPTAPP, INSPECT, PROBE, NETWORK and FLOOR (kiosk and Approval Station) hold the design work per app. The app children are left `open` because each spans several screen groups and two deliverables at different times (wireframes before implementation, mockups before GA), so each will split into per-group leaves. Their phases follow when each app's work starts: COMMON and RPTAPP P1a, INSPECT and FLOOR P1b, PROBE P2, NETWORK P3.

Alternatives considered:
- **Split by fidelity (all wireframes, all mockups).** Rejected: approvals and GA gates are per app (D-017 app milestones), so per-app children match the gating.
- **SETUP as its own child.** Merged into COMMON with the shell frame, because Shop Setup screens sit in the shell frame and are designed together with it.

## Open questions

- **Wireframe-ID convention (added):** screen IDs `SCR.<APP>.<NAME>`, wireframes `WF.<APP>.<NAME>@<rev>`, mockups `MK.<APP>.<NAME>@<rev>`, defined in UX.SCREENS.INV. Implementation leaves cite them in a `Wireframes:` line in `## Design` until maintainers add a `wireframes:` frontmatter field and wire `fid-design screens-gate` into scope.py or CI (a tooling PR, not done here).
- FLOW.PERSONA.SCHEMA can now validate journey-map `screen` values against the inventory (FLOW-side `uses_artifacts`, not editable from here).
- The app list follows D-004; any change to the app family changes the inventory.
- FLOW.PROBE and FLOW.NETWORK are still `open`; UX.SCREENS.PROBE and UX.SCREENS.NETWORK trace only to persona maps until their job maps exist.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [UX.SCREENS.INV](inv/NODE.md) | Screen inventory, wireframe-ID convention and approval records (leaf) | 001 002 003 008 009 | none |
| [UX.SCREENS.COMMON](common/NODE.md) | Shared shell, Shop Setup and first-run screens | 002 003 004 005 006 007 008 | none |
| [UX.SCREENS.RPTAPP](rptapp/NODE.md) | Report app screens | 002 003 004 005 006 007 008 | none |
| [UX.SCREENS.INSPECT](inspect/NODE.md) | Inspect app screens | 002 003 004 005 006 007 008 | none |
| [UX.SCREENS.FLOOR](floor/NODE.md) | Kiosk and Approval Station screens | 002 003 004 005 006 007 008 | none |
| [UX.SCREENS.PROBE](probe/NODE.md) | Probe app screens | 002 003 004 005 006 007 008 | none |
| [UX.SCREENS.NETWORK](network/NODE.md) | Network app screens | 002 003 004 005 006 007 008 | none |
