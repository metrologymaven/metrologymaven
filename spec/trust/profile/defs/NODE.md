---
id: TRUST.PROFILE.DEFS
title: Built-in profile definitions, schema and digest
level: 3
parent: TRUST.PROFILE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-profile]
built_pr: 290
satisfies: [REQ-TRUST.PROFILE-001, REQ-TRUST.PROFILE-006]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.PROFILE.DEFS: Built-in profile definitions, schema and digest

## Purpose

Defines the three governance profiles as data: a published schema, the parameter values of Lean, Regulated and Assistive-only, the narrow set of tenant parameters a site may set on a built-in profile, and the canonical profile document whose digest is recorded in every Result Package and journal session. Everything else in the PROFILE subsystem reads these definitions; nothing here enforces them.

## Scope

**In:**
- Profile document schema (JSON Schema 2020-12) and Rust types
- Parameter values for the three built-ins
- Tenant parameter file: extra meanings (`X_` codes) and, for Regulated, the required meaning sequence
- Instantiation (built-in + tenant parameters) into one canonical document and its SHA-256 digest
- The `profile/profile.json` member written into packages (consumed by `TRUST.PKG.CONTENT`)

**Out:**
- Selecting the active profile (`TRUST.PROFILE.SELECT`)
- Enforcing it on the bus, export and ceremony (`TRUST.PROFILE.GATE`)
- The regulatory mapping document (`TRUST.PROFILE.MAP`)
- Tenant-defined profiles beyond the three (not offered; needs a decision)

## Requirements

- **REQ-TRUST.PROFILE.DEFS-001** The crate shall define exactly three built-in profiles named `lean`, `regulated` and `assistive_only`, each with a version and a complete parameter set.
- **REQ-TRUST.PROFILE.DEFS-002** The parameter set shall contain at least: `required_meanings` (ordered list), `release_requires_signatures` (bool), `allow_software_keys` (bool), `agent_writes` (`branch | proposals_only`), `program_release_by_other` (`agent_authored | always`) and `tenant_meanings` (list).
- **REQ-TRUST.PROFILE.DEFS-003** The Regulated built-in shall set `release_requires_signatures` true, default `required_meanings` to `[AUTHORED, APPROVED]`, `allow_software_keys` false and `program_release_by_other` `always` (D-008); Assistive-only shall set `agent_writes` to `proposals_only`; Lean shall set `release_requires_signatures` false.
- **REQ-TRUST.PROFILE.DEFS-004** A tenant parameter file shall be able to set only `tenant_meanings` and, for Regulated, `required_meanings`; any other key shall fail instantiation with `profile.param_forbidden`.
- **REQ-TRUST.PROFILE.DEFS-005** If a tenant `required_meanings` list for Regulated omits `APPROVED` or names a code not in the catalogue, then instantiation shall fail with `profile.param_invalid`.
- **REQ-TRUST.PROFILE.DEFS-006** The profile digest shall be the SHA-256 of the RFC 8785 canonical JSON of the instantiated document, and the same inputs shall yield the same digest on every platform.

## Acceptance criteria

- **AC-1** Given no tenant file, when each built-in is instantiated, then the documents equal `fixtures/profile/builtin/{lean,regulated,assistive_only}.json` byte for byte and their digests equal the pinned values.
- **AC-2** Given `fixtures/profile/regulated-with-xqa.json` (adds `X_QA_RELEASE`, sequence `[AUTHORED, X_QA_RELEASE, APPROVED]`), then instantiation succeeds and the digest differs from the plain Regulated digest.
- **AC-3** (error path) Given a tenant file setting `allow_software_keys: true` for Regulated, then `profile.param_forbidden`.
- **AC-4** (error path) Given a Regulated sequence without `APPROVED`, then `profile.param_invalid`.
- **AC-5** Given AC-1 on Windows, Linux and macOS CI, then all digests are equal.

## Design

- **Registry command(s):** none.
- **Data structures:** `ProfileName` enum; `ProfileDoc { name, version, params: Params }`; `Params { required_meanings: Vec<MeaningCode>, release_requires_signatures, allow_software_keys, agent_writes, program_release_by_other, tenant_meanings: Vec<TenantMeaning> }`; `TenantParams`.
- **Algorithm:** start from the built-in constant, merge allowed tenant keys, validate, canonicalise (RFC 8785), hash.
- **Errors:** `profile.param_forbidden`, `profile.param_invalid`, `profile.malformed`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-profile` (module `defs`); no dependency on CORE so it can be read by the verifier.

## Test fixtures

- `fixtures/profile/builtin/*.json` and tenant files (synthetic, CC-BY-4.0), shared with `TRUST.ESIG.MEANING`.

## Build prompt

> Create crate `fid-profile` with module `defs`. Define three built-in governance profiles (`lean`, `regulated`, `assistive_only`) as constants with a published JSON Schema. Parameters: `required_meanings` (ordered), `release_requires_signatures`, `allow_software_keys`, `agent_writes` (`branch|proposals_only`), `program_release_by_other` (`agent_authored|always`), `tenant_meanings`. Values: Regulated = signatures required, `[AUTHORED, APPROVED]`, hardware keys only, release-by-other always (D-008); Assistive-only = `proposals_only`; Lean = signatures not required. A tenant parameter file may set only `tenant_meanings` and (Regulated) `required_meanings`, which must include `APPROVED` and use known codes; errors `profile.param_forbidden`, `profile.param_invalid`, `profile.malformed`. Instantiate to one canonical document (RFC 8785) and SHA-256 digest. Tests: the five acceptance criteria. Constraints: CONSTITUTION §8, D0, no CORE dependency, builds for wasm32.

## Open questions

- Whether Lean should allow software keys only, or both, is set to "both" (`allow_software_keys` true) pending user feedback.
- Values not fixed by REQ-TRUST.PROFILE.DEFS-003, as built in `fid-profile` (#290). They are part of every profile digest, so changing one changes the digests:
  - Lean `agent_writes: branch`: REQ-TRUST.PROFILE.GATE.QUERY-004 makes Lean all-permissive.
  - Regulated `agent_writes: branch`: GATE.QUERY-004 lists only `require_signature_before_release` and `software_key_signing` as Regulated gates.
  - Assistive-only `release_requires_signatures: false`, `allow_software_keys: true`, `required_meanings: []`: GATE.QUERY-004 lists only `agent_direct_write: disabled` for Assistive-only.
  - Lean `required_meanings: []`: signatures are optional in Lean (CONSTITUTION §3, D-035).
  - Lean and Assistive-only `program_release_by_other: agent_authored`: REQ-CMM.EXEC.TICKET-002 requires a different releasing human for agent-authored programs, or always under Regulated.
  - `version: 1` for each built-in.
- Limits chosen by the build (#290), also pinned by the schemas: at most 8 `required_meanings` (REQ-TRUST.ESIG.MULTI-001), at most 32 tenant meanings, labels 1 to 64 and descriptions 1 to 400 Unicode scalar values, tenant file and document at most 128 KiB, JSON nesting depth at most 8. A Regulated sequence may not repeat a code. Labels and descriptions must be NFC and may not hold C0/C1 controls, DEL or bidirectional controls (U+202A to U+202E, U+2066 to U+2069). Tenant meanings are listed in the document sorted by code.

## Children

_Leaf: none._
