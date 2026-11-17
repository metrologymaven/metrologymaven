---
id: UNC.RDL.FORMAT
title: RDL dataset record format and listing
level: 3
parent: UNC.RDL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-001, REQ-UNC.RDL-002]
covers: []
depends_on: [KRN]
side_effect: R0
determinism: D0
cli: [fid rdl list-dataset, fid rdl show-dataset]
mcp: [rdl_list_dataset, rdl_show_dataset]
journal: [rdl.list_dataset@1, rdl.show_dataset@1]
updated: 2026-11-16
---

# UNC.RDL.FORMAT: RDL dataset record format and listing

## Purpose

Defines the `RdlDataset` record shared by every tier (X external, G generated, O open suites, B blind): identity (tier, aim, content digest), inputs as content-addressed blobs with their unit, reference outputs with test uncertainty and precision, generator and cross-check records, provenance, licence and visibility. It also defines the public view that strips content a licence does not let us redistribute, and the two read commands.

## Scope

**In:**
- `rdl-dataset/v1` schema, canonical form and digest
- ID grammar `rdl:<tier>/<aim>/<case>/<nnnn>`
- Visibility `public | reference_only | blind` and the `public_view` function
- Unit naming for inputs (for example mm for TraCIM and NIST data) and the declared conversion to SI
- `rdl.list_dataset@1` and `rdl.show_dataset@1` (R0)

**Out:**
- Importing tier X data (owned by `UNC.RDL.EXT`); generating tier G (owned by `UNC.RDL.GEN`)
- Release sets (owned by `UNC.RDL.FREEZE`)
- Blind custody (owned by `UNC.RDL.BLIND`)

## Requirements

- **REQ-UNC.RDL.FORMAT-001** Every dataset record shall carry an ID matching `rdl:<tier>/<aim>/<case>/<nnnn>`, tier in {X, G, O, B}, the kernel aim (algorithm ID without version), inputs as SHA-256 blob references with unit, reference outputs with test uncertainty, licence (SPDX ID or `LicenseRef-*`), provenance and visibility.
- **REQ-UNC.RDL.FORMAT-002** The dataset digest shall be SHA-256 over the RFC 8785 canonical record including the input blob digests, and shall be identical on every platform.
- **REQ-UNC.RDL.FORMAT-003** If a record's licence does not permit redistribution, then its visibility shall be `reference_only`, and `public_view` shall return the record with input and reference-output content removed while keeping all digests, the source reference and the licence.
- **REQ-UNC.RDL.FORMAT-004** If a record is tier B, then `public_view` shall return only its ID, aim and digest.
- **REQ-UNC.RDL.FORMAT-005** If a record omits licence, provenance or test uncertainty, or names an input unit outside {m, mm, rad, deg, µm}, then validation shall fail with a typed error naming the field.
- **REQ-UNC.RDL.FORMAT-006** When `rdl.list_dataset@1` is called, the system shall return records' public views, filterable by tier and aim, sorted by ID.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/format/valid/*.json` (one record per tier), when validated and canonicalized, then each digest equals `fixtures/rdl/format/digests.txt` on wasm32, x86-64 and ARM64.
- **AC-2** Given `fixtures/rdl/format/valid/x_reference_only.json`, when `public_view` is applied, then inputs and reference outputs are absent and digests, source reference and licence are present.
- **AC-3** Given the tier B fixture, when `fid rdl show-dataset --id <id> --json` runs, then only ID, aim and digest are returned.
- **AC-4** (error path) Given each file in `fixtures/rdl/format/invalid/`, when validated, then it fails with the field named in the file name.
- **AC-5** Given the valid fixtures loaded, when `fid rdl list-dataset --tier G --json` runs, then only tier G records are returned in ID order.
- **AC-6** (replay) Given a journal with the calls from AC-3 and AC-5, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `rdl.list_dataset@1` (R0; filters `tier`, `aim`; returns `Vec<PublicRecord>`), `rdl.show_dataset@1` (R0; `id`; returns `PublicRecord`).
- **Data structures:** `RdlDataset { id, tier, aim, inputs: Vec<BlobRef{digest, unit, role}>, reference: { outputs: Vec<RefOutput{name, kind, value_hex, test_uncertainty_hex}>, precision_digits }, generator: Option<GeneratorRecord>, cross_check: Vec<CrossCheck>, provenance, licence, visibility }`; reference values stored as IEEE-754 hex plus an optional decimal string with more digits.
- **Algorithm:** bounded JSON parse (record ≤ 4 MB, depth ≤ 32), validation in field order, canonicalization, digest; blobs stored by digest in the RDL store.
- **Errors:** `schema_invalid{field}`, `bad_id`, `bad_unit`, `missing_licence`, `missing_provenance`, `missing_test_uncertainty`, `dataset_not_found`.
- **Determinism:** D0.
- **Performance budget:** list 10,000 records in under 200 ms [unverified until measured].
- **Crate:** `fid-rdl`.

## Test fixtures

`fixtures/rdl/format/`: small synthetic records per tier (CC-BY-4.0) and invalid variants. No external data is needed.

## Build prompt

> In crate `fid-rdl`, define the `rdl-dataset/v1` schema and `RdlDataset`: ID `rdl:<tier>/<aim>/<case>/<nnnn>`; tier X|G|O|B; kernel aim; inputs as SHA-256 blob references with unit (m, mm, rad, deg, µm) and role; reference outputs with kind, value (IEEE-754 hex plus optional long decimal) and test uncertainty; precision digits; optional generator record and cross-check list; provenance; licence (SPDX or LicenseRef); visibility `public | reference_only | blind`. Digest = SHA-256 over RFC 8785 canonical JSON including blob digests.
> Provide `validate`, `public_view` (reference_only: remove input and reference-output content, keep digests, source reference, licence; tier B: only ID, aim, digest) and two R0 registry commands, `rdl.list_dataset@1` (CLI `fid rdl list-dataset`, filters tier and aim, sorted by ID, returns public views) and `rdl.show_dataset@1` (`fid rdl show-dataset`), both journaled and supporting `--json`. Bounded parsing (≤ 4 MB, depth ≤ 32); typed errors naming the field.
> Tests with `fixtures/rdl/format/`: digests identical on wasm32, x86-64, ARM64; public view of a reference_only record; tier B view; invalid files fail with their field; tier filter; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
