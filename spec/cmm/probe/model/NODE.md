---
id: CMM.PROBE.MODEL
title: Probe build-up model and configuration digest
level: 3
parent: CMM.PROBE
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-001, REQ-CMM.PROBE-002]
covers: [PCD.PRB.01]
depends_on: [CORE]
side_effect: W2
determinism: D0
cli: [fid cmm define-probe, fid cmm show-probe]
mcp: [cmm_define_probe, cmm_show_probe]
journal: [cmm.define_probe@1]
updated: 2026-11-16
---

# CMM.PROBE.MODEL: Probe build-up model and configuration digest

## Purpose

Defines the probe configuration entity: the ordered build-up from the head mount through extensions and stylus to one or more tips (single ball, star, disc), with dimensions and materials, a stable ID, a version and a content digest. The digest is what run tickets, validators, simulation reports and qualification records bind to, so it must change exactly when a geometric, material or sensor parameter changes and never otherwise.

## Scope

**In:**
- `ProbeConfig` type, its build-up chain and nominal tip offsets and directions derived from the chain
- Validation of dimensions at entry (positive, finite, bounded)
- Versioning: every edit creates a new version with a new digest; earlier versions stay readable
- Digest computation over the canonical form
- `cmm.define_probe@1` (W2) and `cmm.show_probe@1` (R0)

**Out** (and which node owns it instead):
- Head angle sets and head kinematics (owned by `CMM.PROBE.HEAD`; a ProbeConfig references a head by digest)
- Scanning-probe and non-contact sensor parameters (owned by `CMM.PROBE.SENSOR`; referenced by digest)
- Qualified offsets and effective radii (owned by `CMM.PROBE.QUAL`)
- Rack port assignment (owned by `CMM.PROBE.RACK`)

## Requirements

- **REQ-CMM.PROBE.MODEL-001** The system shall represent a probe configuration as an ordered chain of a head reference, zero to eight extensions, one stylus and one or more tips, with a stable ID (UUIDv7 when new, preserved when imported) and a version number starting at 1.
- **REQ-CMM.PROBE.MODEL-002** The system shall store every length in SI metres as f64 together with its authored value and unit, applying exactly one journaled rounding per conversion.
- **REQ-CMM.PROBE.MODEL-003** If any tip diameter, disc thickness, stylus length, shank diameter or extension length is non-positive, non-finite or greater than the configured maximum (default 1.0 m), then the system shall reject the configuration with error `PROBE_DIMENSION_INVALID` naming the element index and field, and shall journal nothing.
- **REQ-CMM.PROBE.MODEL-004** The system shall support star tips with two to eight arms, each arm given as a unit direction vector (norm within 1e-12 of 1) and an arm length, and shall reject a star whose arm directions are not unit vectors or whose two arms coincide within 1e-9 rad.
- **REQ-CMM.PROBE.MODEL-005** When a configuration is defined or edited, the system shall compute its nominal tip offsets (from the head reference point, in the head frame) by summing the chain element vectors in chain order.
- **REQ-CMM.PROBE.MODEL-006** The system shall compute the configuration digest as SHA-256 over the RFC 8785 canonical JSON of all geometric, material, head-reference and sensor-reference fields, excluding the display name, comments and version number.
- **REQ-CMM.PROBE.MODEL-007** When an existing configuration is edited, the system shall create a new version with a new digest only if the digest changes, and shall keep every earlier version readable by `(id, version)`.
- **REQ-CMM.PROBE.MODEL-008** The system shall type the display name and comments of a configuration as untrusted text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/probe/star5_config.json` (head reference, one 50 mm extension, 40 mm stylus, five-arm star with 2 mm balls), when `fid cmm define-probe --file star5_config.json --json` runs, then the result holds a UUIDv7 `probe_id`, `version: 1`, five tips whose offsets equal `star5_expected_offsets.json` bit for bit, and a digest equal to the fixture's `expected_digest`.
- **AC-2** Given the AC-1 configuration with only the display name changed, when defined again as an edit, then the digest is unchanged, no new version is created and the command reports `UNCHANGED`.
- **AC-3** Given the AC-1 configuration with one tip diameter changed from 2.0 mm to 2.5 mm, when defined as an edit, then `version: 2` is created with a different digest, and `fid cmm show-probe --id <id> --version 1 --json` still returns the original.
- **AC-4** (error path) Given fixture `fixtures/cmm/probe/bad_negative_stylus.json` (stylus length −0.04 m), when `fid cmm define-probe` runs, then it fails with `PROBE_DIMENSION_INVALID` naming element 2 field `length_m`, and the journal head is unchanged.
- **AC-5** (error path) Given a star whose arm 3 direction has norm 1.001, when defined, then it fails with `PROBE_STAR_ARM_INVALID`.
- **AC-6** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL` and the digests match.

## Design

- **Registry commands:**
  - `cmm.define_probe@1`, W2. Params: `config: ProbeConfigInput`, `edit_of: Option<ProbeId>`. Result: `{probe_id, version, digest, tips: [TipNominal], status: CREATED | NEW_VERSION | UNCHANGED}`.
  - `cmm.show_probe@1`, R0. Params: `probe_id`, `version: Option<u32>` (default latest). Result: `ProbeConfig`.
- **Data structures:**
  - `ProbeConfig { id, version, name: UntrustedText, comments: UntrustedText, head_ref: Digest, sensor_ref: Option<Digest>, chain: Vec<ChainElement>, digest }`.
  - `ChainElement = Extension { length_m, diameter_m, material } | Stylus { length_m, shank_diameter_m, material } | Tip { tip_id, kind: Ball | Disc { thickness_m } , diameter_m, material, direction } | Star { arms: Vec<StarArm> }`; `StarArm { tip_id, direction: [f64; 3], length_m, tip }`.
  - `TipNominal { tip_id, offset_m: [f64; 3], direction: [f64; 3] }`.
  - Authored quantities as `Quantity { value_m: f64, authored: Decimal, unit }`.
- **Algorithm:** the chain axis is −Z of the head frame. Offsets are summed element by element in chain order (fixed-order f64 additions, no FMA). Star arm offsets add `direction * length_m` to the stylus end. No transcendental functions are used.
- **Errors:** `PROBE_DIMENSION_INVALID { element, field }`, `PROBE_STAR_ARM_INVALID { arm }`, `PROBE_CHAIN_EMPTY`, `PROBE_DUPLICATE_TIP_ID`, `PROBE_NOT_FOUND`.
- **Determinism:** D0. Pure f64 additions and multiplications in a fixed order; canonical JSON digest.
- **Performance budget:** define or show in < 5 ms for a 16-tip configuration [unverified until measured].
- **Crate:** `fid-cmm-probe`.

## Test fixtures

Synthetic JSON configurations in `fixtures/cmm/probe/` (`star5_config.json`, `star5_expected_offsets.json`, `bad_negative_stylus.json`, `bad_star_arm.json`), written by hand for this leaf and licensed CC-BY-4.0. Expected offsets are computed once with exact decimal arithmetic and stored.

## Build prompt

> In the Rust crate `fid-cmm-probe`, implement the probe build-up model and two registry commands:
> `cmm.define_probe@1` (side-effect W2, journaled) and `cmm.show_probe@1` (R0). CLI: `fid cmm define-probe`, `fid cmm show-probe`, both with `--json`; MCP: `cmm_define_probe`, `cmm_show_probe` (generated from the registry, not hand-written).
> Types: `ProbeConfig` (id UUIDv7, version, untrusted name and comments, head and sensor references by digest, chain of Extension, Stylus, Tip or Star elements), `TipNominal`.
> Requirements: lengths in SI metres f64 with authored value and unit; reject non-positive, non-finite or > 1.0 m (configurable) dimensions with `PROBE_DIMENSION_INVALID {element, field}` and journal nothing; stars of 2-8 arms with unit directions (|norm − 1| ≤ 1e-12) and distinct directions; nominal tip offsets by summing chain vectors along −Z of the head frame in chain order; digest = SHA-256 over RFC 8785 JCS of all geometric, material, head-ref and sensor-ref fields (not name, comments or version); an edit creates a new version only if the digest changes, and every version stays readable.
> Acceptance tests: the six criteria in the spec node `CMM.PROBE.MODEL` with fixtures in `fixtures/cmm/probe/` (star5 define, name-only edit unchanged, dimension edit creates v2, negative stylus rejected, bad star arm rejected, exact replay IDENTICAL).
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`; no FMA; no std HashMap or HashSet; NaN is an error; determinism class D0.

## Open questions

- The default maximum dimension (1.0 m) is a sanity bound, not a domain figure; a domain expert may lower it.

## Children

_Leaf: none._
