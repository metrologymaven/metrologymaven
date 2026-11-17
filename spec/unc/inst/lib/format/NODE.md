---
id: UNC.INST.LIB.FORMAT
title: Profile library release format and profile reads
level: 4
parent: UNC.INST.LIB
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.LIB-002, REQ-UNC.INST.LIB-003, REQ-UNC.INST.LIB-004]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.INST.MPE.DERIVE, HAL.IDL.PROFILE, CORE.JRN.CANON]
side_effect: [R0, R0]
determinism: D0
cli: [fid unc list-profile, fid unc show-profile]
mcp: [unc_list_profile, unc_show_profile]
journal: [unc.list_profile@1, unc.show_profile@1]
updated: 2026-11-17
---

# UNC.INST.LIB.FORMAT: Profile library release format and profile reads

## Purpose

Defines how the curated instrument-profile library ships: a release manifest that lists each profile (an `instrument-profile/v1` descriptor paired with its `spec_derived` `instrument-model/v1` model and the MPE derivation record), the validator that a release must pass, the rule that superseded profiles stay resolvable by digest, and the two read commands. Content is supplied by `UNC.INST.LIB.CURATE`.

## Scope

**In:**
- `instrument-profile-library/v1` manifest: library semver, release digest, entries `{profile_id, profile_digest, model_digest, derivation_digest, device_class, maker, model_name, source_url, retrieved_on, superseded_by}`
- Release validator: every entry `spec_derived`, `uec_max` ≤ 1, derivation record present, source URL and retrieval date present, text fields bounded
- Resolution by digest across all shipped library versions
- `unc.list_profile@1` and `unc.show_profile@1` (R0)

**Out:**
- Profile content and source review (owned by `UNC.INST.LIB.CURATE`)
- Deriving the model from datasheet values (owned by `UNC.INST.MPE.DERIVE`)
- The site's own instruments (owned by `UNC.INST.LIB.REG`)

## Requirements

- **REQ-UNC.INST.LIB.FORMAT-001** If a library entry's model is not kind `spec_derived`, its profile's `uec_max` exceeds 1, or it lacks a derivation record digest from `UNC.INST.MPE.DERIVE`, then release validation shall fail with `profile_not_spec_derived`, `uec_exceeds_source` or `derivation_missing` naming the entry.
- **REQ-UNC.INST.LIB.FORMAT-002** If an entry lacks a source URL or retrieval date, or any text field exceeds 4 096 bytes, then release validation shall fail with `source_missing` or `text_too_long`.
- **REQ-UNC.INST.LIB.FORMAT-003** When `unc.list_profile@1` is called with optional filters `device_class` and `maker`, the system shall return, sorted by profile ID, the profile ID, device class, maker, model name, source URL, retrieval date and digest of each current (not superseded) entry.
- **REQ-UNC.INST.LIB.FORMAT-004** When `unc.show_profile@1` is called with a digest of any profile in any shipped library version, including superseded ones, the system shall return that entry and, if superseded, the digest that supersedes it.
- **REQ-UNC.INST.LIB.FORMAT-005** When a library version changes an entry's values or source, the validator shall require a new profile semver and digest, a `superseded_by` link on the old entry, and a higher library semver, and fail with `version_not_bumped` otherwise.
- **REQ-UNC.INST.LIB.FORMAT-006** Maker and model names shall be stored as untrusted text and never interpreted (CONSTITUTION C6).

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/lib/release_1_0_0/` (4 invented profiles, one per device class), when validated, then it passes and its release digest equals `release_1_0_0.sha256`.
- **AC-2** Given that release installed, when `fid unc list-profile --device-class tracker --json` runs, then exactly the tracker entry is returned with all listed fields.
- **AC-3** (error paths) Given `fixtures/unc/inst/lib/bad/` (an `observation` model, `uec_max: 2`, no derivation record, no retrieval date, 5 000-byte maker), then validation fails with the matching code for each.
- **AC-4** (supersession) Given `release_1_1_0/` that corrects one entry, when `fid unc show-profile --digest <old digest> --json` runs, then the old entry is returned with `superseded_by` set; `list-profile` returns only the new one.
- **AC-5** (versioning) Given a copy of 1.1.0 that changes a value without bumping the profile semver, then validation fails with `version_not_bumped`.

## Design

- **Registry command(s):** `unc.list_profile@1` (R0; `device_class: Option`, `maker: Option`; returns `Vec<ProfileSummary>`); `unc.show_profile@1` (R0; `digest | profile_id`; returns `ProfileEntry`).
- **Data structures:** `ProfileLibrary { version, entries: Vec<ProfileEntry>, digest }`; `ProfileEntry` as in Scope. Library data ships with the application as read-only data files; all shipped versions are loaded so old digests resolve.
- **Algorithm:** load and validate every shipped version in semver order; build a digest index (BTreeMap); validate each entry's descriptor through `HAL.IDL.PROFILE` and its model through `UNC.INST.SCHEMA`; release digest = SHA-256 over RFC 8785 canonical manifest.
- **Errors:** `profile_not_spec_derived`, `uec_exceeds_source`, `derivation_missing`, `source_missing`, `text_too_long`, `version_not_bumped`, `profile_not_found`.
- **Determinism:** D0.
- **Performance budget:** load 1 000 profiles in under 200 ms [unverified until measured].
- **Crate:** `fid-unc-inst` (module `library`).

## Test fixtures

`fixtures/unc/inst/lib/`: project-authored releases with invented makers (`Example Metrology`), invented MPE values and derivation records, bad variants (CC-BY-4.0). No real datasheet values.

## Build prompt

> In crate `fid-unc-inst`, module `library`, define `instrument-profile-library/v1`: a manifest with library semver and entries `{profile_id, profile_digest, model_digest, derivation_digest, device_class, maker, model_name (untrusted text ≤ 4 096 bytes), source_url, retrieved_on, superseded_by}`, release digest = SHA-256 over RFC 8785 canonical JSON. Validate each entry: descriptor via `fid-hal-idl` `profile` validator, model via the `fid-unc-inst` schema validator, kind must be `spec_derived` and `uec_max` ≤ 1, derivation record (from `unc.derive_instrument_model@1`) present; errors `profile_not_spec_derived`, `uec_exceeds_source`, `derivation_missing`, `source_missing`, `text_too_long`, `version_not_bumped` (changed values without new profile semver, `superseded_by` link and higher library semver). Load all shipped versions so superseded digests resolve.
> Registry commands `unc.list_profile@1` (R0; CLI `fid unc list-profile`; filters device class and maker; current entries sorted by ID) and `unc.show_profile@1` (R0; by digest or ID; shows `superseded_by`). `--json`. Tests AC-1..AC-5 with `fixtures/unc/inst/lib/`. No panic, `unwrap`, `expect`; BTreeMap only (CONSTITUTION §8).

## Open questions

- Whether library updates ship only with application releases or also as separately installable data packs (which would be A0 under CONSTITUTION §3.1) is for ECO's owners; this leaf assumes application releases only.

## Children

_Leaf: none._
