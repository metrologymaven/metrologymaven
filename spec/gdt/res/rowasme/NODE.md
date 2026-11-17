---
id: GDT.RES.ROWASME
title: P1a ASME Y14.5-2018 DMT row set
level: 3
parent: GDT.RES
kind: module
status: decision
decision: D-016
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-009]
covers: []
depends_on: [GDT.RES.SCHEMA, GDT.RES.ROWFLOW]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GDT.RES.ROWASME: P1a ASME Y14.5-2018 DMT row set

## Purpose

The expert-authored DMT rows for the P1a scope: every ASME Y14.5-2018 (with Y14.5.1-2019 where used) callout and modifier combination needed by `GDT.DIM`, `GDT.FORM`, `GDT.LOC` and `GDT.DRF` in P1a. Per D-009 these rows are written by paid expert author(s) and signed off by two experts per row; agents may build structure and process around them but may not author interpretations. This node defines the row **inventory and acceptance process**, not the row content.

## Scope

**In:**
- The P1a row inventory (which keys must exist): the plain dimensional checks of `GDT.DIM` (distance, caliper, angle, diameter, radius, scalar, min/max, point deviation); flatness, straightness, circularity, cylindricity; parallelism and perpendicularity to planar datums; position of holes and pins RFS and MMC (surface interpretation default, axis as variant); two-point size and Rule #1 envelope; DRF from planar datums (concept doc 05 §13 P1a Track B list)
- Per-row acceptance gate: schema-valid, lint-clean, linked ORS cases green, two-expert sign-off (D-009)
- Row-set release as a versioned, digested `DmtTable`
- Defaults profile `dp:asme-y14.5-2018@1.x` content (key values only)

**Out** (and which node owns it instead):
- Row schema (owned by `GDT.RES.SCHEMA`)
- Proposal and sign-off workflow (owned by `GDT.RES.ROWFLOW`)
- ISO rows (owned by `GDT.RES.ROWISO`)
- ORS case content (owned by `UNC.ORS`)

## Requirements

- **REQ-GDT.RES.ROWASME-001** The P1a row set shall contain one row (with variants where needed) for every callout and modifier combination in the P1a inventory listed in Scope, and the support matrix shall show each as `supported` or `partial` before P1a exit.
- **REQ-GDT.RES.ROWASME-002** Every row in the released set shall carry review state `signed_off` with two distinct credentialed reviewers recorded (D-009).
- **REQ-GDT.RES.ROWASME-003** Every `supported` row shall link at least one ORS case that is green in CI at the release commit.
- **REQ-GDT.RES.ROWASME-004** No row in the set shall be authored or adopted by an agent principal; agent-proposed drafts shall be rewritten or adopted by a human author before review.

## Interfaces

- **Offers:** the released P1a `DmtTable` and the ASME defaults profile content.
- **Consumes:** schema (GDT.RES.SCHEMA); workflow (GDT.RES.ROWFLOW); ORS cases (UNC.ORS); purchased standards texts (D-016).

## Decomposition hints

One child per tolerance family inventory (FORM rows, ORIENT rows, POS rows, SIZE rows, DRF rows), each owned by an expert author, plus a RELEASE task.

## Open questions

- **Blocked on D-016:** rows cannot be written until the project has lawful access to the ASME Y14.5-2018 and Y14.5.1-2019 texts for each author and both reviewers, and it is undecided who buys them and under what licence terms.
- Concentricity and symmetry rows for legacy-edition ASME drawings: whether to have them at all is an expert call (raised on `GDT.LOC`).

## Children

_None yet: status `decision`._
