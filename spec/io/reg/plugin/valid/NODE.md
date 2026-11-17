---
id: IO.REG.PLUGIN.VALID
title: Plugin output validation and plugin provenance
level: 4
parent: IO.REG.PLUGIN
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG.PLUGIN-004, REQ-IO.REG.PLUGIN-005]
covers: []
depends_on: [IO.REG.PLUGIN.ABI, IO.REG.PROV, IO.QIF.JSON.CMD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.REG.PLUGIN.VALID: Plugin output validation and plugin provenance

## Purpose

Everything a plugin returns is checked before it can reach the document: geometry records must satisfy the same invariants first-party readers guarantee (finite coordinates, valid indices, declared unit), and QIF fragments must validate against the generated QIF-JSON schema. The provenance of a plugin import also names the plugin, its version and the digest of its package.

## Scope

**In:**
- Record checks: finite f64 coordinates and normals, index bounds for meshes, non-empty point sets, declared unit, counts within limits
- QIF fragments: validation through the QIF-JSON import path (schema, form, identifiers)
- Drawing-page and table records: dimension and count bounds
- Provenance extension: `plugin { id, version, package_sha256 }` added to `SourceProvenance`

**Out:**
- The contract and hosts (siblings)
- Package signing and installation (ECO)

## Requirements

- **REQ-IO.REG.PLUGIN.VALID-001** Every record returned by a plugin shall be checked before staging, and any record that violates a geometry, index, unit or count invariant shall fail the whole import with `plugin_output_invalid` naming the record index and invariant.
- **REQ-IO.REG.PLUGIN.VALID-002** QIF fragments returned by a plugin shall pass the QIF-JSON schema and form checks of `IO.QIF.JSON.CMD`, or the import shall fail with `plugin_output_invalid`.
- **REQ-IO.REG.PLUGIN.VALID-003** The provenance of a plugin import shall record the plugin ID, plugin version and package SHA-256 in addition to the fields of `IO.REG.PROV`.
- **REQ-IO.REG.PLUGIN.VALID-004** Validation shall apply equally to contract readers shipped with the product (origin `first_party`), so their output gets no exemption.

## Acceptance criteria

- **AC-1** Given a test reader returning a valid point record, then the import succeeds and provenance holds plugin ID, version and package digest.
- **AC-2** (error path) Given a record with a NaN coordinate, then `plugin_output_invalid` naming `finite_coordinates`.
- **AC-3** (error path) Given a mesh with a face index beyond its vertex count, then `plugin_output_invalid` naming `index_bounds`.
- **AC-4** (error path) Given a QIF fragment with a decimal written as a JSON number, then `plugin_output_invalid`.
- **AC-5** Given the in-process drawing reader (`IO.REG.PLUGIN.DRAW`) returning a page with zero width, then `plugin_output_invalid`.

## Design

- **Registry commands:** none (library called by the contract adapters).
- **Data structures:** `Invariant` enum (`FiniteCoordinates`, `IndexBounds`, `UnitDeclared`, `CountLimit`, `PageBounds`); `PluginProvenance { id, version, package_sha256 }`.
- **Algorithm:** check each record as it arrives (streaming), before it enters the `StageSink`; QIF fragments go through the QIF-JSON validation path.
- **Errors:** `plugin_output_invalid{record, invariant}`.
- **Determinism:** D0.
- **Performance budget:** validation < 10% of import time for 10^7 points [unverified until measured].
- **Crate:** `fid-io-plugin` (module `valid`).

## Test fixtures

In-process test readers emitting crafted bad records, under `fixtures/io/reg/plugin/valid/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-plugin`, add module `valid`: check every contract record before staging (finite coordinates and normals, mesh index bounds, declared unit, count limits, drawing-page bounds) and fail the whole import with `plugin_output_invalid{record, invariant}`; validate QIF fragments through the QIF-JSON schema and form checks; extend provenance with plugin ID, version and package SHA-256. Apply the same checks to first-party contract readers. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
