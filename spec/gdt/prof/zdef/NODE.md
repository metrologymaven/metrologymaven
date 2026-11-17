---
id: GDT.PROF.ZDEF
title: Profile zone definitions
level: 3
parent: GDT.PROF
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.PROF-002]
covers: []
depends_on: [GDT.PROF.DEV, GDT.ZONE.ZTYPE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.PROF.ZDEF: Profile zone definitions

## Purpose

Defines the profile zone around the nominal for each zone definition the characteristic can state: uniform bilateral, unilateral (outside or inside), unequally disposed (a stated outward share), and non-uniform (a zone width varying along the feature between stated points). Each definition becomes a per-point allowed interval on the signed deviation. The definitions are standards interpretation (ASME unequally disposed and non-uniform rules, ISO equivalents), so they come from DMT rows.

## Scope

**In:**
- Per-point allowed interval [lower_i, upper_i] for each zone definition
- Non-uniform width interpolation between stated points along the feature, as the row defines
- Reporting which definition was applied

**Out** (and which node owns it instead):
- Deviations (owned by `GDT.PROF.DEV`)
- Uniform offset zone shape (owned by `GDT.ZONE.ZTYPE`)
- Evaluating the value (owned by `GDT.PROF.LINE`, `GDT.PROF.SURF`)

## Requirements

- **REQ-GDT.PROF.ZDEF-001** The system shall convert each supported zone definition into a per-point allowed interval on the signed deviation, using the parameters stated on the characteristic and the rules of the resolved row.
- **REQ-GDT.PROF.ZDEF-002** The ResultRecord shall name the zone definition applied and its parameters.
- **REQ-GDT.PROF.ZDEF-003** If the characteristic states a zone definition the row marks unsupported, then the system shall return `UNSUPPORTED`.

## Interfaces

- **Offers:** `zone_intervals(definition, deviations) -> [Interval]` to LINE and SURF.
- **Consumes:** deviations (GDT.PROF.DEV); offset shape (GDT.ZONE.ZTYPE); rows.

## Decomposition hints

UNIF (uniform, unilateral and unequally disposed; a leaf once row semantics are signed off), NONUNIF (non-uniform interpolation).

## Open questions

- Non-uniform zone interpolation along a surface is not fully specified by a single parameter [unverified]; needs an expert row. Owned by `GDT.PROF.ZDEF.NONUNIF`, which stays open until a signed-off row exists.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.PROF.ZDEF.UNIF | Constant-width zone intervals and dispatch (leaf) | 001 002 003 | none |
| GDT.PROF.ZDEF.NONUNIF | Non-uniform profile zone intervals | 001 002 | none |
