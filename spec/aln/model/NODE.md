---
id: ALN.MODEL
title: Alignment object model, hierarchy and save/recall
level: 2
parent: ALN
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ALN-001, REQ-ALN-006, REQ-ALN-008]
covers: [GOM.ALN.08, PCD.ALN.04, PW.ALN.08, SA.XFM.02]
depends_on: [CORE, GEO]
cli: [fid align list-*, fid align get-*, fid align set-active, fid align set-parent, fid align create-*, fid align edit-group, fid align save-*, fid align recall-*, fid align adopt-*, fid align delete-*]
mcp: [align_list_*, align_get_*, align_set_active, align_set_parent, align_create_*, align_edit_group, align_save_*, align_recall_*, align_adopt_*, align_delete_*]
journal: [align.set_active@*, align.set_parent@*, align.create_*, align.edit_group@*, align.save_*, align.recall_*, align.adopt_*, align.delete_*]
updated: 2026-11-16
---

# ALN.MODEL: Alignment object model, hierarchy and save/recall

## Purpose

Defines the `Alignment` element that every alignment method produces: what it stores, how it sits in the dependency graph, how alignments chain into a hierarchy (alignment groups, nested frames), which alignment is active, how alignments are saved to and recalled from a library, and how agent-proposed alignments are tagged and adopted. Solvers live in sibling nodes; this node owns the container they all write into, so that every method is re-runnable and traceable in the same way.

## Scope

**In:**
- The `Alignment` element type: method ID and version, inputs (element references and selections), parameters, resulting transform, residual-report reference, parent alignment, provenance
- Registration of alignment elements as dependency-graph nodes and recompute on input change (through CORE's graph)
- Alignment hierarchy: parent/child chaining, composed transform to the root frame, alignment groups (several data sets moved together)
- Named frames: create, nest, build from an existing alignment (SA-style frames)
- Active-alignment selection and the rule that every result records the alignment it was evaluated in
- Save and recall of alignments to a project-local library, and re-targeted recall on new data
- Agent provenance tags, the "proposed" state, and the human adoption command (CONSTITUTION C5)
- Deletion and invalidation rules when a parent alignment changes or is removed

**Out** (and which node owns it instead):
- Any solver: prealignment and picked pairs (`ALN.PRE`), best-fit (`ALN.BFIT`), 3-2-1 and RPS (`ALN.RPS`), element and datum alignment (`ALN.DATUM`), point-pair and matrix transforms (`ALN.XFM`)
- Residual statistics and conditioning diagnostics (`ALN.QA`)
- The generic dependency graph, journal and branches (`CORE`)
- Frame and transform math types (`GEO`)
- Datum mobility during tolerance evaluation (`GDT`)
- Multi-station network frames (`LVM`)

## Requirements

- **REQ-ALN.MODEL-001** The `Alignment` element shall store method ID with version, input references, parameters with units, resulting rigid (or similarity) transform, parent alignment reference, residual-report reference and provenance, and its stable ID shall follow CONSTITUTION §5.
- **REQ-ALN.MODEL-002** When any input element, parameter or parent alignment of an alignment changes, the system shall recompute that alignment and every alignment and result downstream of it as one journaled recompute.
- **REQ-ALN.MODEL-003** The system shall compose chained alignments so that the transform from any alignment to the root frame equals the ordered product of its ancestors' transforms, with round-trip error at most 1e-12 m on points within 10 m of the origin [unverified].
- **REQ-ALN.MODEL-004** If an alignment edit would create a cycle in the hierarchy, then the system shall reject the command with a typed error and journal nothing.
- **REQ-ALN.MODEL-005** Every result element shall record the ID and content digest of the alignment it was evaluated in, and the system shall refuse to evaluate a result when no alignment is active and the result needs one.
- **REQ-ALN.MODEL-006** The system shall support alignment groups in which one transform is applied to several data elements together.
- **REQ-ALN.MODEL-007** When the user saves an alignment, the system shall store its method, parameters and input selectors (not its data) so that recall on new data re-runs the method and produces a new alignment element linked to the saved definition.
- **REQ-ALN.MODEL-008** When an agent principal creates or modifies an alignment, the system shall tag it `proposed` with agent provenance, and results that depend on it shall not be releasable until a human principal runs the adopt command.
- **REQ-ALN.MODEL-009** If the principal issuing an adopt command is the agent that authored the proposal, then the system shall reject the command (CONSTITUTION C5).

## Interfaces

- **Offers:** the `Alignment` and `Frame` element types; `align.list_alignments`, `align.get_alignment`, `align.set_active@1`, `align.save_alignment@1`, `align.recall_alignment@1`, `align.adopt_alignment@1` (human principal only; not projected to agent tools), `align.delete_alignment@1`. A solver-facing contract: each solver child returns a transform plus residual report and this node wraps it into an element.
- **Consumes:** CORE dependency graph, journal and principals; GEO `Transform` and frame types; `ALN.QA` residual report type.

## Decomposition hints

ELEM (element type and graph registration), HIER (chaining, groups, frames, cycle checks), ACTIVE (active alignment and result stamping), LIB (save and recall), ADOPT (provenance and adoption).

## Split rationale

The split follows the **lifecycle of one alignment element**: what it is and how it recomputes (ELEM), where it sits relative to others (HIER), how results bind to it (ACTIVE), how its definition outlives its data (LIB), and who may vouch for it (ADOPT). Each child owns one set of commands, so the CLI/MCP surface splits cleanly, and four of the five are small enough to finish as leaves now.

Alternatives considered:
- **Merge ACTIVE into HIER.** Rejected: stamping is called by every evaluating domain and has a different consumer set than hierarchy editing.
- **Put adoption in CORE as a generic mechanism.** Rejected for now: C5 is generic, but the release-block closure and the agent-edit-reverts-to-proposed rule are alignment-specific; CORE can later absorb the state machine if other domains repeat it.
- **Split HIER further now** (chain, groups, frames, delete). Deferred to its own expansion; it is the one child that still needs GEO.FRAME's leaf shape.

## Open questions

- Does recall-on-new-data belong here or in AUTO's re-targeted replay? Current split: the alignment definition is here; AUTO replays commands that call it.

- Frontmatter `cli`/`mcp`/`journal` now also list the HIER commands (`set-parent`, `create-frame`, `create-group`, `edit-group`), which the level-2 interface omitted.
- `align.adopt_alignment` is classed W3 in `ALN.MODEL.ADOPT` (human-only request on MCP); reviewers should confirm this reading of CONSTITUTION §3.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ALN.MODEL.ELEM | Alignment element type and dependency-graph registration (leaf) | 001 002 | none |
| ALN.MODEL.HIER | Alignment hierarchy, groups, frames and deletion | 003 004 006 | GOM.ALN.08, PW.ALN.08, SA.XFM.02 |
| ALN.MODEL.ACTIVE | Active alignment and result stamping (leaf) | 005 | none |
| ALN.MODEL.LIB | Alignment save and recall library (leaf) | 007 | PCD.ALN.04 |
| ALN.MODEL.ADOPT | Agent provenance and human adoption of alignments (leaf) | 008 009 | none |
