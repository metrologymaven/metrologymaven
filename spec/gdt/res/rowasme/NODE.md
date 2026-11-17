---
id: GDT.RES.ROWASME
title: P1a ASME Y14.5-2018 DMT row set
level: 3
parent: GDT.RES
kind: module
status: split
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
updated: 2026-11-17
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

- **Offers:** the released P1a `DmtTable` and the ASME defaults profile content; the inventory file format `dmt-inventory/v1` (INV) and the dialect release gate (GATE), both reused by `GDT.RES.ROWISO`.
- **Consumes:** schema (GDT.RES.SCHEMA); workflow (GDT.RES.ROWFLOW); ORS cases and run results (UNC.ORS, read as artifacts); standards texts held under D-016.

## Decomposition hints

One child per tolerance family inventory (FORM rows, ORIENT rows, POS rows, SIZE rows, DRF rows), each owned by an expert author, plus a RELEASE task.

## Open questions

- **Applied D-016 (accepted 2026-11-17):** the project buys one licence of ASME Y14.5-2018 and Y14.5.1-2019 per paid author; community reviewers hold their own copies (or a partner's institutional access, D-009 option C) and attest "edition held" on every review (`GDT.RES.ROWFLOW.STATE`). The node is no longer blocked; its content children remain open because the rows are expert work, never agent-written.
- Purchasing the author licences is a steering-group budget action outside the tree; `GDT.RES.ROWASME.GATE` checks only what the project records (roster basis and attestations), not the purchase itself.
- Concentricity and symmetry rows for legacy-edition ASME drawings: whether to have them at all is an expert call (raised on `GDT.LOC`); routed to `GDT.RES.ROWASME.POS`.

## Split rationale

Machinery versus content again (as in `GDT.RES`): INV (the machine-readable inventory and its coverage check) and GATE (the set-level release gate) are agent-buildable leaves tested on synthetic rows; the content children follow the decomposition hint, one per tolerance family, plus DPROF for the defaults profile values, because each family is authored and reviewed as a unit and maps to one consuming GDT subsystem. Hint RELEASE is GATE; the per-row adoption command already exists in `GDT.RES.ROWFLOW.ADOPT`. DIM rows are folded into SIZE: both serve `GDT.DIM` and share authors.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.RES.ROWASME.INV | DMT inventory file and coverage check (leaf) | 001 | none |
| GDT.RES.ROWASME.GATE | Dialect row-set release gate (leaf) | 001 002 003 004 | none |
| GDT.RES.ROWASME.SIZE | ASME dimensional and size rows | 001 002 003 004 | none |
| GDT.RES.ROWASME.FORM | ASME form rows | 001 002 003 004 | none |
| GDT.RES.ROWASME.ORIENT | ASME orientation rows | 001 002 003 004 | none |
| GDT.RES.ROWASME.POS | ASME position rows | 001 002 003 004 | none |
| GDT.RES.ROWASME.DRF | ASME datum and DRF rows | 001 002 003 004 | none |
| GDT.RES.ROWASME.DPROF | ASME Y14.5-2018 defaults profile values | 001 002 004 | none |
