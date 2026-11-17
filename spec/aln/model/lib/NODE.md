---
id: ALN.MODEL.LIB
title: Alignment save and recall library
level: 3
parent: ALN.MODEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL-007]
covers: [PCD.ALN.04]
depends_on: [ALN.MODEL.ELEM, CORE.REG, GEO.SEL]
side_effect: W2
determinism: D0
cli: [fid align save-alignment, fid align recall-alignment]
mcp: [align_save_alignment, align_recall_alignment]
journal: [align.save_alignment@1, align.recall_alignment@1]
updated: 2026-11-16
---

# ALN.MODEL.LIB: Alignment save and recall library

## Purpose

Saves an alignment's definition (method, parameters and input selectors, never its data) to a project-local library, and recalls a definition on new data by re-running its method, producing a new alignment linked to the saved definition. This is the PC-DMIS-style save and recall, and the building block AUTO uses for re-targeted replay.

## Scope

**In:**
- `AlignmentDefinition` record and its digest
- The project-local alignment library (named definitions)
- `align.save_alignment@1` and `align.recall_alignment@1`
- Selector resolution against the current document through GEO.SEL
- Method dispatch by algorithm ID through the registry

**Out** (and which node owns it instead):
- Cross-project or shared libraries and packaging (`CORE.PRJ`, `ECO`)
- Re-targeted replay of whole sessions (`AUTO`, `CORE.RPL`); it calls recall as an ordinary command
- The solvers (`ALN.PRE`, `ALN.BFIT`, `ALN.RPS`, `ALN.DATUM`, `ALN.XFM`)

## Requirements

- **REQ-ALN.MODEL.LIB-001** When `align.save_alignment` runs on an alignment with a name, the system shall store an `AlignmentDefinition` holding algorithm ID with version, SI and authored parameters, one selector per input, the parent alignment's definition name or `root`, and the source alignment digest, and shall store no measured data.
- **REQ-ALN.MODEL.LIB-002** If the name already exists in the library and `replace` is not set, then the save shall fail with `name_exists` and journal nothing.
- **REQ-ALN.MODEL.LIB-003** When `align.recall_alignment` runs, the system shall resolve every selector against the current document, run the definition's method through the registry, and create a new alignment (new UUIDv7) whose `recalled_from` field holds the definition name and digest.
- **REQ-ALN.MODEL.LIB-004** If any selector resolves to no element or to more than one element, then the recall shall fail with `selector_unresolved` listing each failing selector and journal nothing.
- **REQ-ALN.MODEL.LIB-005** If the definition's algorithm version is not registered, then the recall shall fail with `method_unavailable` and shall not run another version.
- **REQ-ALN.MODEL.LIB-006** When a definition is recalled on the same data it was saved from, a method of determinism class D0 shall reproduce the source transform bit-identically.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/recall-block` (two synthetic block meshes, the second moved by a known rigid transform, with LSQ planes named `top`, `side`, `end` on each), when A1 (a 3-2-1 on run 1) is saved as `block321` and recalled on run 2, then the new alignment's transform differs from the known move by at most 1e-12 m and 1e-12 rad, and `recalled_from` holds `block321` and the definition digest.
- **AC-2** Given `recall-block`, when `block321` is recalled on run 1, then the transform is bit-identical to A1's.
- **AC-3** (error path) Given run 2 with the `end` plane deleted, when `block321` is recalled, then the command fails with `selector_unresolved` naming `end` and journals nothing.
- **AC-4** (error path) Given `block321` saved, when it is saved again without `replace`, then the command fails with `name_exists`.
- **AC-5** Given the saved definition file, then it contains no point, mesh or coordinate data other than parameters.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry commands:** `align.save_alignment@1` (W2; params `alignment: AlignmentId`, `name: String` (1-128 chars, `[A-Za-z0-9_.-]`), `replace: bool`); `align.recall_alignment@1` (W2; params `name`, optional `bindings: Map<input_role, ElementId>` overriding selectors, optional `parent: AlignmentId`). Results: definition digest; new alignment ID.
- **Data structures:** `AlignmentDefinition { name, algorithm_id, params, inputs: Vec<{ role, selector: geo::Selector }>, parent: DefinitionName | root, source_digest, digest }`. Selectors are GEO.SEL semantic queries (by element name, type and tag), never index sets into one data set.
- **Algorithm:** save derives a selector per input from the element's name and type (or the user-supplied selector); recall resolves, then dispatches through CORE.REG to the solver command named by the algorithm ID, as an ordinary W2 command call so that it journals like a direct run plus the `recall` event.
- **Errors:** `name_exists`, `invalid_name`, `selector_unresolved`, `method_unavailable`, `not_found`.
- **Determinism:** D0 for the leaf's own logic; the produced alignment carries the solver's class.
- **Performance budget:** recall overhead (excluding the solver) under 5 ms [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`fixtures/aln/model/recall-block`: seeded generator (seed 2) producing two 20,000-triangle block meshes 200 x 100 x 50 mm, the second rotated 5 degrees about Z and shifted (10, -5, 2) mm, with named LSQ planes. Project-generated, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement the project-local alignment library: `AlignmentDefinition`, `align.save_alignment@1` and `align.recall_alignment@1` (both W2; CLI `fid align save-alignment|recall-alignment` with `--json`; MCP `align_save_alignment`, `align_recall_alignment`).
> Requirements: save stores algorithm ID and version, parameters (SI and authored), one GEO semantic selector per input, the parent definition name or root, and the source digest; never measured data. Existing name without `replace` gives `name_exists`. Recall resolves each selector to exactly one element (else `selector_unresolved` listing all failures, nothing journaled), dispatches the solver command by algorithm ID through the registry (unregistered version gives `method_unavailable`, no substitution), and creates a new alignment with `recalled_from` = name plus definition digest. Recall on the source data reproduces a D0 method's transform bit-identically.
> Acceptance: fixture `fixtures/aln/model/recall-block` (seed 2). Tests: recall on moved data recovers the known move within 1e-12 m and 1e-12 rad; recall on the source data is bit-identical; deleted input gives `selector_unresolved`; duplicate save gives `name_exists`; definition file holds no measured data; exact replay IDENTICAL.
> Constraints: no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet; RFC 8785 canonical JSON plus SHA-256 for digests; agents never pass raw file paths (the library lives inside the project container).

## Open questions

- The parent's question (recall here versus AUTO's re-targeted replay) is settled as: the definition and recall command are here; AUTO replays commands that call recall.

## Children

_Leaf: none._
