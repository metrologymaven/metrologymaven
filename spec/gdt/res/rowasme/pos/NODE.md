---
id: GDT.RES.ROWASME.POS
title: ASME position rows
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

# GDT.RES.ROWASME.POS: ASME position rows

## Purpose

The expert-authored ASME Y14.5-2018 DMT rows (with Y14.5.1-2019 where used) for the inventory keys of family POS: position of holes and pins, RFS and MMC, with surface interpretation as the default variant and axis interpretation as a named variant, including patterns under one callout. Under D-009 a paid expert author writes each row and two credentialed reviewers sign it off; under D-016 the author works from a project-purchased licence and each reviewer attests holding the cited edition. Agents may draft structure (keys, empty chains, ORS links) through `gdt.propose_dmt_row@1` on an agent branch, but never the interpretation itself, and no agent-authored text reaches review. This node stays open until the rows are authored; it is content work, not an implementation task.

## Scope

**In:**
- One row per inventory key of family POS (with named variants where the reading has accepted alternatives)
- Clause citations by designator, edition and clause number only; operator chains naming existing algorithm IDs; actual-value definition IDs; decision-rule hints; limitation codes
- Links to ORS cases that exercise each `supported` row
- Review through `GDT.RES.ROWFLOW` with edition-held attestations

**Out** (and which node owns it instead):
- The inventory itself (owned by `GDT.RES.ROWASME.INV`)
- The release gate (owned by `GDT.RES.ROWASME.GATE`)
- Defaults-profile values (owned by `GDT.RES.ROWASME.DPROF`)
- ORS case content (owned by `UNC.ORS.GDT`, `UNC.ORS.SIZE`, `UNC.ORS.DRF`)
- Executing the chains (owned by `GDT.LOC.POS`, `GDT.ZONE.MATL` and `GDT.ZONE.GROUP.PATTERN`)

## Requirements

- **REQ-GDT.RES.ROWASME.POS-001** The table shall contain a row for every inventory key whose family is POS, with status `supported` or `partial` before P1a exit.
- **REQ-GDT.RES.ROWASME.POS-002** Every row shall be authored by a human principal whose roster basis is `paid_author` or `partner`, and shall reach `signed_off` through `GDT.RES.ROWFLOW` with two distinct reviewers who are not the author.
- **REQ-GDT.RES.ROWASME.POS-003** Every review of a row shall carry an edition-held attestation for each cited designator and edition (D-016).
- **REQ-GDT.RES.ROWASME.POS-004** Every `supported` row shall link at least one ORS case that passes at the release commit, and every operator in its chain shall name an algorithm ID present in the kernel manifest.
- **REQ-GDT.RES.ROWASME.POS-005** No row shall contain reproduced standards text; free text shall stay within the `GDT.RES.LINT` limits.

## Interfaces

- **Offers:** signed-off rows of family POS, adopted into the released P1a table by `gdt.adopt_dmt_row@1`.
- **Consumes:** inventory (GDT.RES.ROWASME.INV); workflow commands (GDT.RES.ROWFLOW.PROPOSE, GDT.RES.ROWFLOW.REVIEW, GDT.RES.ROWFLOW.ADOPT); ORS cases (UNC.ORS, read as artifacts).

## Decomposition hints

None for agents. The author may split the family into review batches; each batch is a set of row revisions, not a spec node.

## Open questions

- Surface versus axis precedence for ASME position (concept doc 05 §2.5 example row) must be confirmed against the licensed text by the author and both reviewers; `GDT.ZONE.MATL.BOTH` reads the deciding variant from these rows.
- Concentricity and symmetry for legacy-edition ASME drawings: whether to have rows at all is an expert call (raised on `GDT.LOC`, `GDT.LOC.COAX`, `GDT.LOC.SYM`); if yes they belong to a later edition-specific inventory, not the 2018 set.
- Default objective for a pattern with unequal allowances (`GDT.ZONE.GROUP.PATTERN`) and the actual-size definition for bonus (`GDT.ZONE.MATL.BONUS`) are row content here.
- Blocked on people, not on a decision: the paid author(s) must be contracted and licensed copies bought (D-009, D-016) before any row can be written.

## Children

_None yet: status `open`._
