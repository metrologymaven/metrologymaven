---
id: GDT.ZONE.GROUP.SIMREQ
title: Simultaneous-requirement groups across callouts
level: 4
parent: GDT.ZONE.GROUP
kind: feature
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.GROUP-001, REQ-GDT.ZONE.GROUP-002, REQ-GDT.ZONE.GROUP-004]
covers: []
depends_on: [GDT.ZONE.GROUP.PATTERN, GDT.ZONE.GROUP.IDS, GDT.CHAR.MODEL, GDT.RES.RESOLVE]
uses_artifacts: [UNC.ORS.GDT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.GROUP.SIMREQ: Simultaneous-requirement groups across callouts

## Purpose

Forms groups of characteristics from different callouts that must be evaluated with one shared pose because they reference the same datums in the same order with matching material boundaries, unless the drawing separates them; then solves each group with `GDT.ZONE.GROUP.PATTERN`. Which DRF components must match, and whether grouping is the default or must be stated, differs between dialects and editions, so the rule must come from a DMT field that does not exist yet.

## Scope

**In:**
- Group formation: explicit group labels from the characteristic model, plus default grouping per the resolved row's matching rule
- Separate-requirement overrides (a callout opting out of the default group)
- Solving each formed group through `GDT.ZONE.GROUP.PATTERN` and recording `GroupRef` with kind `simreq`

**Out** (and which node owns it instead):
- The shared-pose solve (owned by `GDT.ZONE.GROUP.PATTERN`)
- Group identity (owned by `GDT.ZONE.GROUP.IDS`)
- The matching rule's content (owned by DMT rows in `GDT.RES.ROWASME` and `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.ZONE.GROUP.SIMREQ-001** When two or more characteristics share an explicit simultaneous-requirement label, the system shall solve them as one group with one shared pose.
- **REQ-GDT.ZONE.GROUP.SIMREQ-002** When the resolved rows define default grouping, the system shall group characteristics whose DRF references match on exactly the components the rows name, and shall exclude any characteristic carrying a separate-requirement override.
- **REQ-GDT.ZONE.GROUP.SIMREQ-003** Each grouped characteristic's ResultRecord shall carry the `GroupRef` with kind `simreq`, and the grouping basis (`explicit_label` or `row_default`) shall be recorded.
- **REQ-GDT.ZONE.GROUP.SIMREQ-004** If two characteristics in a candidate group resolve to rows whose matching rules differ, then the system shall return `simreq_rule_conflict` naming both rows and form no group.
- **REQ-GDT.ZONE.GROUP.SIMREQ-005** The system shall reproduce every ORS-GDT simultaneous-requirement case within its stated tolerance.

## Interfaces

- **Offers:** `form_simreq_groups(chars, resolutions) -> Vec<GroupSpec>` and `solve_simreq(...)`, used by `GDT.LOC.POS`, `GDT.LOC.ORIENT` and `GDT.PROF.SURF.CMD` when a characteristic belongs to a group.
- **Consumes:** shared-pose solve (GDT.ZONE.GROUP.PATTERN); IDs (GDT.ZONE.GROUP.IDS); group labels (GDT.CHAR.MODEL); rows via the resolver (GDT.RES.RESOLVE).

## Decomposition hints

FORM (group formation from labels and row rules; a leaf once the DMT field exists), SOLVE (thin wrapper over PATTERN; probably folds into FORM).

## Open questions

- Blocked on content, not on a decision: `GDT.RES.SCHEMA` has no field for the default simultaneous-requirement matching rule (which DRF components must match: datum features, order, material boundary modifiers). Adding it is a schema change for the RES maintainers; the rule's values are expert content for `GDT.RES.ROWASME` and `GDT.RES.ROWISO` under D-009.
- Grouping across characteristics also needs the evaluation commands to run grouped characteristics together (one command call for a group or a group pre-pass); confirm the command shape with `GDT.LOC.POS`.

## Children

_None yet: status `open`._
