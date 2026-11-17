---
id: UX.ICONS
title: Original icon set and GD&T symbol drawings
level: 2
parent: UX
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001, REQ-UX-006]
covers: []
depends_on: [UX.SYSTEM, UX.TOKENS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX.ICONS: Original icon set and GD&T symbol drawings

## Purpose

An original icon set drawn for this project, plus drawing specifications for geometric dimensioning and tolerancing symbols that the viewer, the reports and the docs render identically. It includes the side-effect class icons and the status glyph shapes that carry meaning without colour. It is separate because icons are the artefact most exposed to trade-dress risk (CONSTITUTION C9) and because GD&T symbols follow standards, so both need their own provenance and review trail.

## Scope

**In:**
- Icon grid, stroke and corner rules, sizes and the icon style guide
- Icons for every role named by `UX.COMP`, `UX.PATTERN` and `UX.SCREENS`, and for registry command icon fields
- Side-effect class icons (R0, R1, W1, W2, W3, X1, I1, D1, M1, S1, A0)
- Status glyph shapes for the six display states and the provenance badge shapes
- GD&T symbol drawing specs: geometric characteristic symbols, modifiers, datum feature and datum target symbols, feature control frame proportions
- Icon provenance records and accessible-name message IDs
- Application and file icons in brand-neutral form until D-002

**Out** (and which node owns it instead):
- Logo and brand marks (`OUTR.BRAND.GUIDE` and `OUTR.BRAND.ASSET`)
- Mapping display states to glyphs at runtime (`VIZ.OVERLAY.STATUS`)
- Rendering feature control frames in the viewport (`VIZ.OVERLAY`) and in reports (`RPT`)
- Icon colours (`UX.TOKENS`)
- Translating accessible names (`I18N`)

## Requirements

- **REQ-UX.ICONS-001** Every icon shall be original vector artwork drawn on the project grid at 16 and 24 logical pixels, and shall stay legible at 16 pixels at 100 % scale in every theme.
- **REQ-UX.ICONS-002** Every icon shall carry a provenance record (author, date, licence and the originality declaration from `UX.SYSTEM`), and any third-party glyph shall also be listed in `ATTRIBUTION.md`.
- **REQ-UX.ICONS-003** The set shall define one icon for each of the eleven side-effect classes, and each shall be distinguishable from the others by shape alone in greyscale.
- **REQ-UX.ICONS-004** The six display-state glyphs shall be distinguishable from each other by shape alone in greyscale at 16 pixels.
- **REQ-UX.ICONS-005** The set shall include a drawing spec for every geometric characteristic symbol and modifier in ASME Y14.5-2018 and ISO 1101:2017, and for datum feature and datum target symbols per ISO 5459:2011, citing designator and clause only and with proportions stated relative to text height.
- **REQ-UX.ICONS-006** A GD&T symbol drawn from the spec shall have the same geometry in UI and report outputs at equal text height, within 0.5 % of text height [unverified tolerance].
- **REQ-UX.ICONS-007** Every icon used without a visible text label shall have an accessible-name message ID.
- **REQ-UX.ICONS-008** If a registry command names an icon that is not in the set, then the icon-set check shall report the command ID and the missing name.

## Interfaces

- **Offers:** the icon set and GD&T symbol specs in the design-system package; icon names for the registry's icon field; glyph shapes for display states and side-effect classes.
- **Consumes:** `UX.SYSTEM` (package, originality declaration, review), `UX.TOKENS` (icon colour roles). Interface notes (no `depends_on`): the registry manifest (CORE) supplies the list of command icon names for REQ-UX.ICONS-008; `VIZ.OVERLAY.STATUS` maps states to the glyphs defined here; `RPT` renders the GD&T specs in print; `GDT` supplies the list of supported symbols per dialect.

## Decomposition hints

GRID (style guide, grid, sizes), CORESET (general UI icons), CLASS (side-effect class and status glyphs), GDT (GD&T symbol specs), PROV (provenance and icon-set checks).

## Open questions

- Whether standard GD&T symbol shapes raise any copyright or trade-dress question is assumed not to (they are functional notation), but this is [unverified].
- A GD&T symbol font versus vector drawings is an implementation choice left to leaves; it affects REQ-UX.ICONS-006.

## Children

_None yet: status `open`._
