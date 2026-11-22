---
id: GDT.RES.ROWISO.DPROF
title: ISO defaults profiles and decision-rule hint
level: 4
parent: GDT.RES.ROWISO
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWISO-002, REQ-GDT.RES.ROWISO-003]
covers: []
depends_on: [GDT.RES.ROWISO.STUBS, GDT.RES.ROWASME.INV, GDT.RES.ROWASME.GATE, GDT.RES.ROWFLOW.PROPOSE, GDT.RES.ROWFLOW.REVIEW, GDT.RES.ROWFLOW.ADOPT]
uses_artifacts: [UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# GDT.RES.ROWISO.DPROF: ISO defaults profiles and decision-rule hint

## Purpose

The expert-authored DMT rows for ISO 8015 and ISO 14253-1: the ISO defaults profiles (`dp:iso-…`) per edition set and the default decision-rule hint from ISO 14253-1 passed to UNC. In P1b these rows replace the `unsupported` placeholders from `GDT.RES.ROWISO.STUBS` key by key. Under D-009 a paid or partner expert authors each row and two credentialed reviewers sign it off; under D-016 the author works from a project-purchased licence of the cited edition and each reviewer attests holding it. Agents may draft structure only; no interpretation is agent-written. The node stays open until the rows are authored.

## Scope

**In:**
- Rows for the P1b ISO inventory keys of family DPROF, one key per edition where editions differ
- Clause citations by designator, edition and clause number only; chains naming existing algorithm IDs; ORS links
- Review through `GDT.RES.ROWFLOW` with edition-held attestations, release through `GDT.RES.ROWASME.GATE` with the ISO inventory

**Out** (and which node owns it instead):
- Placeholders and the counterpart map (owned by `GDT.RES.ROWISO.STUBS`)
- ASME rows (owned by `GDT.RES.ROWASME`)
- ORS case content (owned by `UNC.ORS`)
- Executing the chains (owned by `GDT.CHAR.DPROF` and `UNC`)

## Requirements

- **REQ-GDT.RES.ROWISO.DPROF-001** Every row shall name the edition set it applies to, and rows for different editions of the same standard shall be distinct keys.
- **REQ-GDT.RES.ROWISO.DPROF-002** Every row promoted from `unsupported` shall be authored by a human with roster basis `paid_author` or `partner` and reach `signed_off` with two distinct reviewers, each attesting the cited edition (D-009, D-016).
- **REQ-GDT.RES.ROWISO.DPROF-003** Every `supported` row shall link at least one ORS case that passes at the release commit, and the P1b ISO release shall pass `GDT.RES.ROWASME.GATE` with the P1b ISO inventory.
- **REQ-GDT.RES.ROWISO.DPROF-004** No row shall contain reproduced standards text, and no row text or citation shall be sent to an external service.

## Interfaces

- **Offers:** signed-off ISO rows of family DPROF.
- **Consumes:** placeholders and ISO inventory (GDT.RES.ROWISO.STUBS); inventory format and gate (GDT.RES.ROWASME.INV, GDT.RES.ROWASME.GATE); workflow commands (GDT.RES.ROWFLOW); ORS cases (UNC.ORS, read as artifacts).

## Decomposition hints

None for agents.

## Open questions

- Profiles are not DMT rows; the same representation question as `GDT.RES.ROWASME.DPROF` applies (row-like key so the ROWFLOW workflow and gate cover it).
- ISO 8015 is not in D-016's list of texts; confirm with the steering group that it is bought with the P1b set [unverified that the defaults profile needs it].
- Blocked on people, not on a decision: authors, reviewers and licences for the cited editions (D-009, D-016).

## Children

_None yet: status `open`._
