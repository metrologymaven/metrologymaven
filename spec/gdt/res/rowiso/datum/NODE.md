---
id: GDT.RES.ROWISO.DATUM
title: ISO 5459 datum and datum-system rows
level: 4
parent: GDT.RES.ROWISO
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.RES.ROWISO-002, REQ-GDT.RES.ROWISO-003]
covers: []
depends_on: [GDT.RES.ROWISO.STUBS, GDT.RES.ROWASME.INV, GDT.RES.ROWASME.GATE, GDT.RES.ROWFLOW.PROPOSE, GDT.RES.ROWFLOW.REVIEW, GDT.RES.ROWFLOW.ADOPT]
uses_artifacts: [UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWISO.DATUM: ISO 5459 datum and datum-system rows

## Purpose

The expert-authored DMT rows for ISO 5459 (2011 and 2024): datum features, datum systems, common datums and datum-feature simulation for both editions as distinct keys. In P1b these rows replace the `unsupported` placeholders from `GDT.RES.ROWISO.STUBS` key by key. Under D-009 a paid or partner expert authors each row and two credentialed reviewers sign it off; under D-016 the author works from a project-purchased licence of the cited edition and each reviewer attests holding it. Agents may draft structure only; no interpretation is agent-written. The node stays open until the rows are authored.

## Scope

**In:**
- Rows for the P1b ISO inventory keys of family DATUM, one key per edition where editions differ
- Clause citations by designator, edition and clause number only; chains naming existing algorithm IDs; ORS links
- Review through `GDT.RES.ROWFLOW` with edition-held attestations, release through `GDT.RES.ROWASME.GATE` with the ISO inventory

**Out** (and which node owns it instead):
- Placeholders and the counterpart map (owned by `GDT.RES.ROWISO.STUBS`)
- ASME rows (owned by `GDT.RES.ROWASME`)
- ORS case content (owned by `UNC.ORS`)
- Executing the chains (owned by `GDT.DRF`)

## Requirements

- **REQ-GDT.RES.ROWISO.DATUM-001** Every row shall name the edition set it applies to, and rows for different editions of the same standard shall be distinct keys.
- **REQ-GDT.RES.ROWISO.DATUM-002** Every row promoted from `unsupported` shall be authored by a human with roster basis `paid_author` or `partner` and reach `signed_off` with two distinct reviewers, each attesting the cited edition (D-009, D-016).
- **REQ-GDT.RES.ROWISO.DATUM-003** Every `supported` row shall link at least one ORS case that passes at the release commit, and the P1b ISO release shall pass `GDT.RES.ROWASME.GATE` with the P1b ISO inventory.
- **REQ-GDT.RES.ROWISO.DATUM-004** No row shall contain reproduced standards text, and no row text or citation shall be sent to an external service.

## Interfaces

- **Offers:** signed-off ISO rows of family DATUM.
- **Consumes:** placeholders and ISO inventory (GDT.RES.ROWISO.STUBS); inventory format and gate (GDT.RES.ROWASME.INV, GDT.RES.ROWASME.GATE); workflow commands (GDT.RES.ROWFLOW); ORS cases (UNC.ORS, read as artifacts).

## Decomposition hints

None for agents.

## Open questions

- The 2011 and 2024 editions differ in datum-establishment defaults [unverified]; each needs its own keys and its own reviewer attestations.
- Common datums (A-B) need `GDT.DRF.COMMON` (P1b, open) before rows can be `supported`.
- Blocked on people, not on a decision: authors, reviewers and licences for the cited editions (D-009, D-016).

## Children

_None yet: status `open`._
