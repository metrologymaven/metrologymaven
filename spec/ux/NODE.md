---
id: UX
title: UX: design system, interaction design and user research
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-031]
covers: []
depends_on: [FLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX: Design system, interaction design and user research

## Purpose

One visual and interaction language for the four apps, the reports, the website and the docs. It is a **design system** of tokens, components, icons, themes and patterns. It includes:
- wireframes and mockups of every primary screen, made before they are built;
- **user research and usability testing** with real metrologists and operators.

VIZ implements the UI. UX decides what it should look like and how it should behave, and proves that with users.

## Scope

**In:**
- Design principles: dense but calm; data first; every number explainable; keyboard-complete; familiar workflows (CONSTITUTION C9) without incumbent trade dress
- Design tokens: colour (including colour-blind-safe deviation palettes), typography, spacing, elevation, motion; light, dark and high-contrast themes
- Component library spec: trees, tables, property grids, legends, palettes, wizards, dialogs, toasts, the "Why?" panel
- An original icon set, including GD&T symbol rendering
- Interaction patterns shared across apps: selection, pick and lasso, undo, command palette, proposal review, operator prompts
- Wireframes and high-fidelity mockups for each primary screen of Inspect, Probe, Network and Report, plus kiosk and Approval Station
- Report visual language (with RPT): layouts, charts, balloons
- In-app onboarding and guidance patterns (with FLOW Shop Setup and tutorials)
- User research: interviews, contextual inquiry, usability tests per release, SUS tracking
- Design contribution workflow: design files in the open, design review in PRs

**Out:**
- Rendering and toolkit implementation (VIZ)
- Brand identity and name (OUTR.BRAND, D-002)
- Accessibility implementation (VIZ.SHELL; UX sets the standard)

## Requirements

- **REQ-UX-001** The project shall publish a versioned design system (tokens, components, icons, patterns) in an open, tool-neutral format, and VIZ and OUTR shall consume its tokens rather than hard-coding values.
- **REQ-UX-002** Deviation colour maps shall default to perceptually uniform, colour-blind-safe palettes, verified with a simulator for the common colour-vision deficiencies.
- **REQ-UX-003** Every primary screen of each app shall have an approved wireframe before its implementation leaf is built, and a reviewed mockup before GA.
- **REQ-UX-004** Light, dark and high-contrast themes shall be supported, and every text/background token pair shall meet WCAG 2.2 AA contrast.
- **REQ-UX-005** Each release shall be usability-tested with at least 5 practising metrologists or operators on the acceptance jobs, and the SUS score and top issues shall be published.
- **REQ-UX-006** The icon set shall be original. No incumbent icons, layouts or trade dress shall be copied (CONSTITUTION C9).
- **REQ-UX-007** Design work shall be contributable in the open (design files in the repository or an open tool), with design review as part of PRs that change UI.

## Interfaces

- **Offers:** the design system, wireframes and mockups, research reports, design review.
- **Consumes:** FLOW (journeys and jobs to design for); feeds VIZ, RPT, OUTR, I18N.

## Decomposition hints

PRINC (design principles), TOKENS (colour, type, spacing, themes), COMP (component library spec), ICONS (icon set and GD&T symbols), PATTERN (interaction patterns), SCREENS (wireframes and mockups per app), REPORT (report visual language), RESEARCH (research and usability testing), CONTRIB (open design workflow).

## Split rationale

The dividing principle is **the layer of the design system**, from rules to evidence:
- UX.SYSTEM holds the rules and the container: principles, writing guide, package format and versioning, consumption contract, open design workflow and design review (the PRINC and CONTRIB hints, merged because principles are enforced through the review and both govern the same versioned package);
- UX.TOKENS and UX.ICONS are the atoms, each with its own measurable obligations (contrast and colour-vision checks; originality and GD&T symbol fidelity);
- UX.COMP and UX.PATTERN are the building blocks: single components, and behaviours that span components and follow FLOW's journeys and the command-bus rules;
- UX.SCREENS and UX.REPORT apply the system to screens and to print;
- UX.RESEARCH produces the evidence, starting in P0 before any screen exists.

Dependencies only point down the layers and to FLOW. No UX child depends on VIZ, RPT or OUTR nodes; what UX needs from them is written as interface notes in each child, so those domains can depend on UX without a cycle.

Alternatives considered:
- **The nine decomposition hints as nine children.** Rejected: PRINC and CONTRIB own the same artefact and process, and nine children leaves no room if the design-tool decision needs its own node.
- **Split by app** (Inspect, Probe, Network, Report). Rejected: REQ-FID-031 asks for one shared design system, so per-app children would duplicate tokens, components and patterns.
- **Components and patterns as one child.** Rejected: patterns depend on FLOW journeys and the command-bus rules (C3, C5) while components do not, and VIZ tests them differently.

## Open questions

- Which open design tool and file format? Options: Penpot (open source) or Figma with exported tokens. Moved to UX.SYSTEM; needs a small decision when its TOOL child is expanded.
- **Dependency-cycle risk.** FLOW currently `depends_on` VIZ and RPT, and UX (and UX.PATTERN, UX.SCREENS, UX.RESEARCH) depend on FLOW. If any VIZ, RPT or OUTR node adds `depends_on` a UX node, scope.py will report a cycle. FLOW's dependency on VIZ and RPT, or the UX dependency on FLOW, needs revisiting when FLOW is split.
- **Ownership boundaries to confirm in other domains** (not editable from here): VIZ.SHELL.THEME lists the "token model and the four built-in themes" (UX.TOKENS assumes VIZ owns the runtime model and UX owns names and values); VIZ.OVERLAY.STATUS's perceptual-distance threshold should match REQ-UX.TOKENS-009; OUTR.BRAND.GUIDE's visual identity should map onto the UX.TOKENS brand tier; RPT.DESIGN.STYLE should adopt UX.REPORT's default style set.
- REQ-UX-003 gates implementation leaves in other domains, which do not yet cite wireframe IDs; see UX.SCREENS.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| UX.SYSTEM | Design principles, design-system package and open design workflow | 001 006 007 | none |
| UX.TOKENS | Design tokens, themes and data colour palettes | 001 002 004 | none |
| UX.ICONS | Original icon set and GD&T symbol drawings | 001 006 | none |
| UX.COMP | Component library specification | 001 004 | none |
| UX.PATTERN | Shared interaction and guidance patterns | 001 | none |
| UX.SCREENS | Wireframes and mockups of primary screens | 003 006 | none |
| UX.REPORT | Report visual language | 001 002 006 | none |
| UX.RESEARCH | User research and usability testing | 005 | none |
