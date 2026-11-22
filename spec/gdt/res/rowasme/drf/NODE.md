---
id: GDT.RES.ROWASME.DRF
title: ASME datum and DRF rows
level: 4
parent: GDT.RES.ROWASME
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWASME-001, REQ-GDT.RES.ROWASME-002, REQ-GDT.RES.ROWASME-003, REQ-GDT.RES.ROWASME-004]
covers: []
depends_on: [GDT.RES.ROWASME.INV, GDT.RES.ROWFLOW.PROPOSE, GDT.RES.ROWFLOW.REVIEW, GDT.RES.ROWFLOW.ADOPT]
uses_artifacts: [UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# GDT.RES.ROWASME.DRF: ASME datum and DRF rows

## Purpose

The expert-authored ASME Y14.5-2018 DMT rows (with Y14.5.1-2019 where used) for the inventory keys of family DRF: datum feature simulation and DRF construction from planar datums (primary, secondary, tertiary), RMB, including the datum simulator each row names. Under D-009 a paid expert author writes each row and two credentialed reviewers sign it off; under D-016 the author works from a project-purchased licence and each reviewer attests holding the cited edition. Agents may draft structure (keys, empty chains, ORS links) through `gdt.propose_dmt_row@1` on an agent branch, but never the interpretation itself, and no agent-authored text reaches review. This node stays open until the rows are authored; it is content work, not an implementation task.

## Scope

**In:**
- One row per inventory key of family DRF (with named variants where the reading has accepted alternatives)
- Clause citations by designator, edition and clause number only; operator chains naming existing algorithm IDs; actual-value definition IDs; decision-rule hints; limitation codes
- Links to ORS cases that exercise each `supported` row
- Review through `GDT.RES.ROWFLOW` with edition-held attestations

**Out** (and which node owns it instead):
- The inventory itself (owned by `GDT.RES.ROWASME.INV`)
- The release gate (owned by `GDT.RES.ROWASME.GATE`)
- Defaults-profile values (owned by `GDT.RES.ROWASME.DPROF`)
- ORS case content (owned by `UNC.ORS.GDT`, `UNC.ORS.SIZE`, `UNC.ORS.DRF`)
- Executing the chains (owned by `GDT.DRF` (DATUM, SIM, BUILD, CHECK))

## Requirements

- **REQ-GDT.RES.ROWASME.DRF-001** The table shall contain a row for every inventory key whose family is DRF, with status `supported` or `partial` before P1a exit.
- **REQ-GDT.RES.ROWASME.DRF-002** Every row shall be authored by a human principal whose roster basis is `paid_author` or `partner`, and shall reach `signed_off` through `GDT.RES.ROWFLOW` with two distinct reviewers who are not the author.
- **REQ-GDT.RES.ROWASME.DRF-003** Every review of a row shall carry an edition-held attestation for each cited designator and edition (D-016).
- **REQ-GDT.RES.ROWASME.DRF-004** Every `supported` row shall link at least one ORS case that passes at the release commit, and every operator in its chain shall name an algorithm ID present in the kernel manifest.
- **REQ-GDT.RES.ROWASME.DRF-005** No row shall contain reproduced standards text; free text shall stay within the `GDT.RES.LINT` limits.

## Interfaces

- **Offers:** signed-off rows of family DRF, adopted into the released P1a table by `gdt.adopt_dmt_row@1`.
- **Consumes:** inventory (GDT.RES.ROWASME.INV); workflow commands (GDT.RES.ROWFLOW.PROPOSE, GDT.RES.ROWFLOW.REVIEW, GDT.RES.ROWFLOW.ADOPT); ORS cases (UNC.ORS, read as artifacts).

## Decomposition hints

None for agents. The author may split the family into review batches; each batch is a set of row revisions, not a spec node.

## Open questions

- Which simulator the rows name for a planar datum feature (for example a constrained tangent fit) and how candidate datum sets are handled are expert content [unverified for the edition].
- Mobility rows (MMB, LMB) are P1b and are not in this P1a inventory; they will need their own inventory entries when `GDT.DRF.MOB` is written.
- Blocked on people, not on a decision: the paid author(s) must be contracted and licensed copies bought (D-009, D-016) before any row can be written.

## Children

_None yet: status `open`._
