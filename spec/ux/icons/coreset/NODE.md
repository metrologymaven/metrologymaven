---
id: UX.ICONS.CORESET
title: Core UI icon set and brand-neutral app and file icons
level: 3
parent: UX.ICONS
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.ICONS-001, REQ-UX.ICONS-007]
covers: []
depends_on: [UX.ICONS.GRID, UX.ICONS.PROV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.ICONS.CORESET: Core UI icon set and brand-neutral app and file icons

## Purpose

The first original set of general UI icons (files, edit, view, selection, measurement entities, alignment, commands and navigation) and brand-neutral application and project-file icons for the four apps until D-002 names the product. Each icon has an index entry with its name, role, accessible-name message ID and English source text.

## Scope

**In:** `icons/index.json` entries and SVG masters for an initial inventory of at least 120 icons [unverified count]; app icons for Inspect, Probe, Network and Report and the project-file icon in brand-neutral form; accessible-name message IDs `icon.<name>`.
**Out:** side-effect class, status and provenance glyphs (`UX.ICONS.CLASS`); GD&T symbols (`UX.ICONS.GDT`); logo and brand marks (OUTR.BRAND); icons added later by component, pattern or screen work (added through the contribution workflow, `UX.SYSTEM.TOOL`).

## Requirements

- **REQ-UX.ICONS.CORESET-001** The index shall list every icon with `name`, `group`, `role` (one-line meaning), `a11y_id` (`icon.<name>`), `a11y_en` (English source text) and `provenance` (path to its record).
- **REQ-UX.ICONS.CORESET-002** The initial inventory shall cover the groups file, edit, view, selection, geometry elements (point, line, plane, circle, cylinder, cone, sphere, slot, surface, curve), alignment, datum, measure, report, run, device and navigation, with at least one icon per group.
- **REQ-UX.ICONS.CORESET-003** Every icon shall pass `fid-design icons-lint` and carry a provenance record accepted by `fid-design icons-prov`.
- **REQ-UX.ICONS.CORESET-004** App and file icons shall contain no product name, letterform of a candidate name or incumbent motif, and shall be marked `brand_neutral = true` until D-002 is accepted with a name.
- **REQ-UX.ICONS.CORESET-005** Every icon whose index entry is marked `labelless = true` shall have a non-empty `a11y_en` of at most 40 characters.

## Acceptance criteria

- **AC-1** Given the shipped `icons/`, when `fid-design icons-lint` and `fid-design icons-prov` run, then both exit 0.
- **AC-2** Given the shipped index, when `fid-design icons-index --root icons --json` runs, then it reports at least one icon per group of REQ-002 and no entry missing a field.
- **AC-3** (error path) Given fixture `fixtures/ux/icons/coreset/no-a11y/` (a labelless icon with empty `a11y_en`), when `icons-index` runs, then it exits 1 with `ICON-A11Y-MISSING <name>`.
- **AC-4** Given the review record for this set, then it shows a result for every trade-dress rule and the originality declaration for each icon.

## Design

- **Command:** `fid-design icons-index --root <icons dir> [--json]` (index completeness and groups). Tooling, nearest class R0.
- **Drawing process:** draw from the role description, not from any incumbent's icon; record references consulted in words in the provenance record.
- **Errors:** `ICON-INDEX-SCHEMA`, `ICON-A11Y-MISSING`, `ICON-GROUP-EMPTY`, `ICON-FILE-MISSING`, `ICON-BRAND-FLAG`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `icons_index`; artwork in the design-system package.

## Test fixtures

`fixtures/ux/icons/coreset/no-a11y/`; the shipped icon set itself is the main fixture; CC-BY-4.0.

## Build prompt

> Draw at least 120 original icons (16 and 24 masters, following `icons/STYLE.md`) covering the groups file, edit, view, selection, geometry elements, alignment, datum, measure, report, run, device and navigation, plus brand-neutral app icons for Inspect, Probe, Network, Report and a project-file icon. Do not look at or trace any competitor icon; work from the one-line role. Add index entries `{name, group, role, a11y_id, a11y_en, labelless, brand_neutral, provenance}` and a provenance record per icon. In the Rust tooling crate `fid-design`, add `fid-design icons-index --root <dir> [--json]` with errors `ICON-INDEX-SCHEMA|ICON-A11Y-MISSING|ICON-GROUP-EMPTY|ICON-FILE-MISSING|ICON-BRAND-FLAG`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.ICONS.CORESET; AC-4 is the design-review record.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; CONSTITUTION C9 (no copied icons).

## Open questions

- Icons that UX.COMP, UX.PATTERN and UX.SCREENS need beyond this inventory are added by contribution PRs; this node cannot depend on them without a cycle.

## Children

_Leaf: none._
