---
id: IO.CAD.STEP.ASM
title: STEP product structure and placements as frames and instances
level: 4
parent: IO.CAD.STEP
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.STEP-005]
covers: []
depends_on: [IO.CAD.STEP.BREP, GEO.CAD.ASSY, GEO.FRAME.TREE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.STEP.ASM: STEP product structure and placements

## Purpose

Reads the STEP product structure (products, their shape representations and the assembly relations between them) and records it as a part tree with one frame per placement. Body coordinates stay as written in each part's own representation; placements are frames, never baked into coordinates.

## Scope

**In:**
- `product`, `product_definition`, `product_definition_shape`, `shape_definition_representation`
- `next_assembly_usage_occurrence` with `context_dependent_shape_representation` and `representation_relationship_with_transformation` / `item_defined_transformation`
- `mapped_item` with `representation_map` (placements inside one representation)
- Product names and IDs as untrusted text; instance path (root to leaf) per occurrence
- Multiple top-level products

**Out:**
- Bodies (`IO.CAD.STEP.BREP`)
- Instance and part model, placement commands (GEO.CAD.ASSY), frame math (GEO.FRAME)

## Requirements

- **REQ-IO.CAD.STEP.ASM-001** For each assembly occurrence, the reader shall create one instance whose frame equals the occurrence's placement relative to its parent, and body coordinates shall remain those of the part's own representation.
- **REQ-IO.CAD.STEP.ASM-002** Each instance shall carry the path of `next_assembly_usage_occurrence` entity numbers from the root product, so that two occurrences of the same part are distinguishable.
- **REQ-IO.CAD.STEP.ASM-003** If a placement's rotation part is not orthonormal within 1e-9 or is improper (mirror), then the reader shall skip that instance and list it in the report with its entity number and cause `placement_unsupported`.
- **REQ-IO.CAD.STEP.ASM-004** If product relations form a cycle, then the import shall fail with `malformed{reason: assembly_cycle}` naming one entity number on the cycle.
- **REQ-IO.CAD.STEP.ASM-005** A file without product structure shall yield one implicit part holding all bodies in the world frame.

## Acceptance criteria

- **AC-1** Given `fixtures/io/cad/step/asm/two_bolts.stp` (one plate, the same bolt placed twice at (10,0,0) mm and (30,0,0) mm), when read, then 3 instances exist, the two bolt instances share one body definition, their frames translate by (0.01,0,0) and (0.03,0,0) m, and the bolt body's vertex coordinates equal the file values × 0.001.
- **AC-2** Given `fixtures/io/cad/step/asm/nested.stp` (3 levels), then each instance's path lists its NAUO entity numbers from the root.
- **AC-3** Given `fixtures/io/cad/step/asm/mirror.stp`, then the mirrored instance is skipped with `placement_unsupported` and the others are imported.
- **AC-4** (error path) Given `fixtures/io/cad/step/asm/cycle.stp`, then the import fails `malformed{reason: assembly_cycle}`.

## Design

- **Registry commands:** none (library, runs inside the CAD worker; instances are created on the host by `IO.CAD.STEP.CMD`).
- **Data structures:** `ProductTree { parts: BTreeMap<u64, PartDef { name: Untrusted, bodies: Vec<BodyIdx> }>, instances: Vec<InstanceRec { path: Vec<u64>, part: u64, placement: Rigid }>, skipped: Vec<Skipped> }`.
- **Algorithm:** build the product graph keyed by entity number; topological sort with cycle detection; compute each relative placement from the two `axis2_placement_3d` items as T = from⁻¹ · to with fixed-order arithmetic; validate orthonormality and determinant +1.
- **Errors:** `malformed{reason: assembly_cycle}`; skips are report entries.
- **Determinism:** D0.
- **Performance budget:** 10^4 occurrences in < 1 s [unverified until measured].
- **Crate:** `fid-io-step`.

## Test fixtures

Hand-written assembly files under `fixtures/io/cad/step/asm/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-step`, read STEP product structure from a `P21File`: products, product definitions, shape representations, NAUO occurrences with transformation relations, and mapped items. Produce a `ProductTree` with part definitions (untrusted names), instances carrying their NAUO entity-number path and a rigid placement relative to the parent (T = from⁻¹·to; reject non-orthonormal or improper rotations as skipped `placement_unsupported`), and fail on cycles with `malformed{reason: assembly_cycle}`. Never bake placements into body coordinates. No product structure → one implicit part in the world frame. KNP-1 arithmetic (no FMA, fixed order). CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-4.

## Open questions

- Mirrored instances need improper transforms, which GEO.FRAME rejects today (see GEO.CAD.ASSY open questions); they are skipped and reported until that changes.

## Children

_Leaf: none._
