---
id: SCAN.MESHOP.PROV
title: Mesh operation contract and synthesized-triangle flag
level: 3
parent: SCAN.MESHOP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-mesh]
satisfies: [REQ-SCAN.MESHOP-001, REQ-SCAN.MESHOP-005, REQ-SCAN.MESHOP-009]
covers: []
depends_on: [GEO.TYPES, SCAN.POLY.QUAL]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.MESHOP.PROV: Mesh operation contract and synthesized-triangle flag

## Purpose

The shared contract every mesh operation in SCAN.MESHOP implements: produce a new mesh version that references the input digest, leave the input untouched, declare a determinism class, attach before/after quality reports, and carry the per-triangle **synthesized** flag that tells comparison which triangles are not backed by measured data. Writing this once keeps the invariants identical across thin, smooth, fill, cut and repair.

## Scope

**In:**
- `MeshOp` trait: inputs, parameters, determinism class, output builder
- `MeshOpOutput`: new mesh, input digest references, parameters record, quality before and after (from `SCAN.POLY.QUAL`)
- Per-triangle `synthesized` attribute and its propagation rules through split, collapse, copy and creation of triangles
- Input-integrity check helper used by every operation's tests

**Out** (and which node owns it instead):
- Each concrete operation (owned by `SCAN.MESHOP.DECIM`, `SMOOTH`, `HOLES`, `CUT`, `REPAIR`, `SEL`)
- Quality metric definitions (owned by `SCAN.POLY.QUAL`)
- Exclusion of synthesized triangles during comparison (owned by `SCAN.CMP.DEV`)

## Requirements

- **REQ-SCAN.MESHOP.PROV-001** The `MeshOp` contract shall produce every result as a new mesh element version whose record lists the digests of all input versions, and shall give the operation no mutable access to input meshes.
- **REQ-SCAN.MESHOP.PROV-002** Every `MeshOp` implementation shall declare its determinism class as D0, or D2 together with a seed parameter that is recorded with the result; any other class shall be rejected at registration.
- **REQ-SCAN.MESHOP.PROV-003** Every output mesh shall carry a per-triangle `synthesized` flag, where a triangle copied unchanged keeps its input flag, a triangle created to bridge or fill area not covered by input triangles is set to true, and a triangle produced by splitting or collapsing input triangles is true if any contributing input triangle was true.
- **REQ-SCAN.MESHOP.PROV-004** An input mesh without a `synthesized` attribute shall be treated as all-false (fully measured).
- **REQ-SCAN.MESHOP.PROV-005** Every `MeshOpOutput` shall include the `SCAN.POLY.QUAL` report of the input and of the output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/mesh_open_cube` and a test operation that copies the mesh and fills the missing face with two new triangles, when it runs through `MeshOp`, then the output has exactly 2 triangles flagged synthesized, the input digest is unchanged, and the output record lists the input digest.
- **AC-2** Given a test operation that splits a synthesized triangle into four, then all four children are flagged synthesized; splitting a measured triangle gives four measured children.
- **AC-3** Given a test operation declaring determinism D3, when it is registered, then registration fails with `scan.mesh.bad_determinism`.
- **AC-4** Given a D2 test operation run twice with the same seed, then output digests match and the seed appears in the result record.
- **AC-5** Given a mesh without the attribute, the output of an identity operation has the attribute with all values false.

## Design

- **API** (`fid-scan-mesh::op`): `trait MeshOp { const DETERMINISM: Determinism; fn run(&self, input: &MeshView, params: &Self::Params, out: &mut MeshOpBuilder) -> Result<(), MeshOpError>; }`; `MeshOpBuilder` exposes `copy_triangle(src)`, `derive_triangle(&[src], verts)`, `create_triangle(verts)` so flag propagation is enforced by construction rather than by each operation.
- **Data:** `synthesized` is a bit-packed per-triangle attribute in GEO.TYPES; `MeshOpOutput { mesh, inputs: [Digest], params_json, quality_before, quality_after, seed? }`.
- **Errors:** `scan.mesh.bad_determinism`, `scan.mesh.invalid_index`.
- **Determinism:** D0 for the contract itself.
- **Crate:** `fid-scan-mesh` (kernel tier, KNP-1).

## Test fixtures

- `fixtures/scan/mesh_open_cube` (shared with SCAN.POLY.QUAL), CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-mesh`, module `op`, implement the `MeshOp` trait and `MeshOpBuilder`. Operations receive a read-only view of input meshes and build output only through `copy_triangle` (keeps the input `synthesized` flag), `derive_triangle` (flag = OR of contributing triangles) and `create_triangle` (flag = true). Inputs without the attribute are treated as all-false. The builder returns `MeshOpOutput` with the new mesh, input digests, canonical parameter JSON, optional seed, and quality reports before and after from `mesh_quality` (module `quality`). Determinism must be D0, or D2 with a recorded seed; registering any other class fails with `scan.mesh.bad_determinism`.
>
> Constraints: KNP-1 (no FMA, no std HashMap/HashSet, fixed-order reductions), no panic/unwrap/unsafe, input meshes never mutated. Tests: the five acceptance criteria with test-only operations on the open-cube fixture.

## Open questions

- Whether vertices moved by smoothing beyond a fraction of the bound should also mark triangles synthesized; currently only spike relocation does (see `SCAN.MESHOP.SMOOTH`).
- The Design places `synthesized` in GEO.TYPES, but GEO.TYPES.MESH has no such channel and this leaf may not edit it, so `fid-scan-mesh` carries the flags beside the mesh (`Synthesized`) and binds them into a derived-mesh digest (`"FIDMOP"` layout, `op` module docs) distinct from the GEO.TYPES element digest. Proposed: GEO.TYPES.MESH adds a `triangle_synthesized` channel to the digest layout, and the derived digest then becomes the element digest.

## Children

_Leaf: none._
