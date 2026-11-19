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
built_in: [fid-rdl]
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
- **Errors:** `schema_invalid{field}`, `bad_id`, `bad_unit`, `missing_licence`, `missing_provenance`, `missing_test_uncertainty`, `dataset_not_found`; the command surfaces add `bad_params{field}` for malformed CLI arguments or MCP parameters (envelope code `usage_error`). Every error carries a field path built only from schema field names and array indices (`inputs.0.unit`, `reference.outputs.2.test_uncertainty_hex`, `record` for the whole record) and a static reason; no error, message or envelope detail ever holds a value or key taken from the input.
- **Pinned format (rdl-dataset/v1):**
  - Top-level keys, all required unless marked: `schema` (`"rdl-dataset/v1"`), `id`, `tier`, `aim`, `inputs`, `reference`, `generator` (optional; required for tier G), `cross_check` (required, may be empty), `provenance`, `licence`, `visibility`. Unknown keys are refused at every level. Optional fields are absent when unset; `null` is refused, except that a `null` licence, provenance, provenance source or test uncertainty counts as missing.
  - ID grammar: `rdl:<tier>/<aim>/<case>/<nnnn>`; tier one of `X G O B` (upper case); aim `module.feature.method`, three segments `[a-z][a-z0-9_]*`, at most 128 bytes (the KRN.MAN.CARD algorithm ID without `@version`); case `[a-z0-9][a-z0-9_-]*`, at most 64 bytes; nnnn exactly four ASCII digits. The ID's tier and aim must equal the `tier` and `aim` fields (`bad_id` otherwise).
  - Input: `{digest, unit, role}`; digest `sha256:` plus 64 lower-case hex digits (fid-types); unit exactly one of `m`, `mm`, `µm` (U+00B5 MICRO SIGN then `m`; `um` and U+03BC are `bad_unit`), `rad`, `deg`; role `[a-z][a-z0-9_]*`, at most 64 bytes; 1 to 256 inputs. Declared conversion to SI: m, mm, µm to m by 1, 1/1000, 1/1000000; rad, deg to rad by 1, pi/180. The conversion is applied where the input is used, not here.
  - Reference: `{outputs, precision_digits}`; 1 to 256 outputs with unique names; `precision_digits` an integer 1 to 1000. Output: `{name, kind, value_hex, value_dec?, test_uncertainty_hex}`; kind one of `length`, `angle`, `dimensionless` with values in SI (m, rad, 1).
  - Hex floats: `0x` followed by exactly 16 lower-case hex digits of the IEEE 754 binary64 bits (upper case and short forms refused); NaN and infinities refused; test uncertainty and cross-check deviation must have the sign bit clear (so -0 is refused). `value_dec` matches `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`, at most 128 bytes; its agreement with `value_hex` is checked by the producer (GEN, EXT), not here.
  - Generator: `{id, version, seed}`; id `[a-z][a-z0-9_.-]*` (128 bytes); version `[A-Za-z0-9.+-]+` (64 bytes); seed a decimal u64 string without leading zeros. Cross-check: `{oracle, outcome, max_deviation_hex}`; outcome `agree` or `disagree`; at most 16.
  - Provenance: `{source, method?}`, printable text without control characters, 1 to 512 bytes; a missing or empty source is `missing_provenance`.
  - Licence: one SPDX ID `[A-Za-z0-9][A-Za-z0-9.+-]*` or `LicenseRef-[A-Za-z0-9.-]+`, at most 64 bytes; expressions are refused. Redistributable licences are the `[policy] redistributable` list of `datasets.toml` (CC0-1.0, CC-BY-4.0, CC-BY-SA-4.0, MIT, Apache-2.0, BSD-3-Clause, ODbL-1.0, PDDL-1.0), matched exactly; every other licence and every `LicenseRef-*` is treated as not redistributable.
  - Visibility rules: `blind` if and only if tier B; `public` only with a redistributable licence (otherwise `schema_invalid{visibility}`).
  - Validation order: the top-level fields in the order listed above (so a record missing both provenance and licence reports provenance), then unknown keys, then the cross-field rules (ID agreement, tier G generator, visibility). The first failure is returned.
  - Bounds: record at most 4 MiB (4,194,304 bytes); nesting depth at most 32, counting every array and object with the record itself at depth 1; duplicate keys refused.
  - Digest: SHA-256 over the RFC 8785 canonical bytes (fid-canon) of the record as listed above (the record holds no `digest` key), written `sha256:<hex>`.
- **Public view (allowlist, fail-closed):** tier B or visibility `blind` gives exactly `{aim, digest, id}`. Visibility `reference_only`, or any licence that is not redistributable, gives `{aim, digest, id, inputs: [{digest}], licence, provenance: {source}, reference_digest, schema, tier, visibility: "reference_only"}`, where `reference_digest` is SHA-256 of the canonical `reference` object; units, roles, outputs, precision, generator, cross-checks and provenance method are dropped. Otherwise the canonical record plus `digest`.
- **Commands:** results are the shared result envelope (AUTO.SURF.ENV, `computed` or `failed`); `--json` prints its canonical bytes, and the MCP tools return the same envelope. CLI: `fid rdl list-dataset [--tier <X|G|O|B>] [--aim <aim>] [--json]`, `fid rdl show-dataset --id <id> [--json]`. Listing order is ascending ID by bytes. Journaling: per CONSTITUTION §3 and D-031, R0 calls go to the Activity Log, not the Record Journal; each call whose command and parameters parse appends `{command, params, result_sha256, surface}`, where `result_sha256` is SHA-256 of the canonical envelope bytes; exact replay reruns the call and compares that digest (`IDENTICAL` or `DIVERGENT`).
- **Determinism:** D0.
- **Performance budget:** list 10,000 records in under 200 ms [unverified until measured].
- **Crate:** `fid-rdl`.

## Test fixtures

`fixtures/rdl/format/`: small synthetic records per tier (CC-BY-4.0) and invalid variants. No external data is needed. Layout, all written by `make_fixtures.py` (stdlib Python, independent of the Rust code): `valid/*.json` (one record per tier, two for tier G so the ID order is tested), `invalid/<field>--<code>[--<note>].json` (the expected error field and code are in the file name), `digests.txt` (`<file> <digest>`), `expected/*.public.json` (canonical public views) and `expected/list-tier-g.json`, `expected/show-tier-b.json` (canonical envelopes of the AC-5 and AC-3 calls). The X and B records only declare synthetic `LicenseRef-Synthetic-*` licences; no third-party data is used (D-038).

## Build prompt

> In crate `fid-rdl`, define the `rdl-dataset/v1` schema and `RdlDataset`: ID `rdl:<tier>/<aim>/<case>/<nnnn>`; tier X|G|O|B; kernel aim; inputs as SHA-256 blob references with unit (m, mm, rad, deg, µm) and role; reference outputs with kind, value (IEEE-754 hex plus optional long decimal) and test uncertainty; precision digits; optional generator record and cross-check list; provenance; licence (SPDX or LicenseRef); visibility `public | reference_only | blind`. Digest = SHA-256 over RFC 8785 canonical JSON including blob digests.
> Provide `validate`, `public_view` (reference_only: remove input and reference-output content, keep digests, source reference, licence; tier B: only ID, aim, digest) and two R0 registry commands, `rdl.list_dataset@1` (CLI `fid rdl list-dataset`, filters tier and aim, sorted by ID, returns public views) and `rdl.show_dataset@1` (`fid rdl show-dataset`), both journaled and supporting `--json`. Bounded parsing (≤ 4 MB, depth ≤ 32); typed errors naming the field.
> Tests with `fixtures/rdl/format/`: digests identical on wasm32, x86-64, ARM64; public view of a reference_only record; tier B view; invalid files fail with their field; tier filter; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
