---
id: IO.DMIS.DIAL
title: Vendor DMIS dialect profiles as data
level: 3
parent: IO.DMIS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-007]
covers: []
depends_on: [IO.DMIS.PARSE.TREE, IO.DMIS.PARSE.LEX]
side_effect: R0
determinism: D0
cli: [fid import dmis-list-dialects]
mcp: [import_dmis_list_dialects]
journal: [import.dmis_list_dialects@1]
built_in: [fid-io-dmis]
updated: 2026-11-22
---

# IO.DMIS.DIAL: Vendor DMIS dialect profiles

## Purpose

Describes vendor deviations from DMIS 5.3 (extra statements, relaxed lexical rules) as versioned, validated data files rather than code forks, so the parser accepts a vendor's files only when a profile is selected and the journal records which profile and version was used.

## Scope

**In:**
- Profile format `fid.dmis-dialect` v1 (TOML): `id`, `version` (semver), `base` (`dmis-5.3`), `title` and `source_note` (untrusted text naming the public document or observed-file basis), `relaxations` (from a closed list of lexer relaxations), `descriptors` (extra statements written in the TREE shape language, each with a class ID prefixed by the profile ID)
- Validation: schema, ID pattern `[a-z0-9-]{2,32}`, no descriptor may redefine a base DMIS major word, shapes must compile
- Loading from the bundled profile directory and from installed industry packs or plugins (install is A0, ECO); a profile that fails validation is not listed
- `import.dmis_list_dialects@1` (R0): ID, version, base, title, origin (`bundled|pack`), statement count
- Turning a selected profile into the `Dialect` value passed to `parse` and recording `{id, version, digest}` for the import's journal entry
- One synthetic test profile; real vendor profiles are data contributions reviewed for clean-room origin

**Out:**
- Grammar engine (`IO.DMIS.PARSE.TREE`), lexer (`IO.DMIS.PARSE.LEX`)
- Installing packs (ECO, A0)
- The import commands that record the profile (`IO.RES.TRANS.DMIS`, `IO.DMIS.PROG.CMD`)

## Requirements

- **REQ-IO.DMIS.DIAL-001** When a dialect profile is selected, the parser shall accept exactly that profile's relaxations and extra statements and no others.
- **REQ-IO.DMIS.DIAL-002** The selected profile's ID, version and SHA-256 digest shall be returned for recording in the journal of every import that used it.
- **REQ-IO.DMIS.DIAL-003** If a profile file fails validation (schema, ID pattern, redefinition of a base major word, shape compile error), then loading shall fail with `dialect_invalid` naming the file and reason, and the profile shall not be listed.
- **REQ-IO.DMIS.DIAL-004** The system shall expose `import.dmis_list_dialects@1` (R0) listing loaded profiles with ID, version, base, title and origin, sorted by ID then version.
- **REQ-IO.DMIS.DIAL-005** If an import names a profile ID and version that is not loaded, then the import shall fail with `dialect_not_found`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/dial/test-vendor.toml` (adds `VNDX/ON` and relaxation `allow_lowercase_labels`), when `fid import dmis-list-dialects --json` runs, then the list holds `test-vendor 1.0.0` with origin `bundled` and 1 statement.
- **AC-2** Given `fixtures/io/dmis/dial/uses_vndx.dmi`, when parsed with no dialect, then `VNDX/ON` is an `Unknown` node; with `test-vendor`, it is a `Statement` of class `test-vendor.vndx_on`.
- **AC-3** (error path) Given `fixtures/io/dmis/dial/redefines_feat.toml` (redefines `FEAT`), then loading fails `dialect_invalid{reason: base_redefinition}` and it is not listed.
- **AC-4** Given a parse with `test-vendor`, then the returned record is `{id: test-vendor, version: 1.0.0, digest}` and the digest equals the file's SHA-256.
- **AC-5** (error path) Given a request for `test-vendor@9.9.9`, then `dialect_not_found`.

## Design

- **Registry commands:** `import.dmis_list_dialects@1` (R0; params `origin?`; result `Vec<DialectInfo>`). CLI `fid import dmis-list-dialects`, MCP `import_dmis_list_dialects`.
- **Data structures:** `DialectProfile`; `DialectInfo { id, version, base, title: Untrusted, origin, statements }`; `DialectUse { id, version, digest }`; `ProfileSet { by_id: BTreeMap<(Id, Version), DialectProfile> }`.
- **Algorithm:** load and validate all profiles at startup (bounded file size 1 MiB); compile descriptors with the TREE shape compiler; the selected profile merges its descriptors into a per-parse registry overlay.
- **Errors:** `dialect_invalid{file, reason}`, `dialect_not_found`.
- **Determinism:** D0.
- **Performance budget:** loading 100 profiles in < 100 ms [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Synthetic profiles and DMIS fragments under `fixtures/io/dmis/dial/` (CC-BY-4.0). No vendor profile is shipped until one is written from public documentation or observed files under the clean-room rule (CONSTITUTION §8).

## Build prompt

> In crate `fid-io-dmis`, implement DMIS dialect profiles: TOML format `fid.dmis-dialect` v1 (id, semver version, base `dmis-5.3`, untrusted title and source note, relaxations from a closed list, extra statement descriptors in the TREE shape language with class IDs prefixed by the profile ID). Validate (schema, ID pattern, no base-major redefinition, shapes compile) → `dialect_invalid{file, reason}`; load from the bundled directory and installed packs; register `import.dmis_list_dialects@1` (R0; CLI `fid import dmis-list-dialects`; MCP `import_dmis_list_dialects`) sorted by id then version; build the `Dialect` overlay for `parse` and return `{id, version, digest}` for journaling; unknown selection → `dialect_not_found`. No vendor text copied. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- Profiles are data loaded from installed packs; installing them follows the A0 plugin and pack rule (CONSTITUTION §3.1, D-007), and listing is R0. This answers the earlier question.
- D-033 does not block DMIS dialects: they are DMIS, an open route; profiles must still cite only public documentation or observed files.

## Children

_Leaf: none._
