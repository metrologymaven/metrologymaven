---
id: UNC.UEC.STMT
title: Uncertainty statement and evidence class types
level: 3
parent: UNC.UEC
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-kunc]
satisfies: [REQ-UNC.UEC-001, REQ-UNC.UEC-004]
covers: []
depends_on: [KRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UNC.UEC.STMT: Uncertainty statement and evidence class types

## Purpose

Defines the `EvidenceClass` enumeration and the `UncertaintyStatement` value type that every ResultRecord carries, with constructor-enforced invariants and a canonical (hashable) serialization. Every other UNC node, GDT, RPT and TRUST write and read uncertainty only through these types, so the invariants (UEC-0 has no numbers; UEC-1..3 always state method, coverage, model and unit) are enforced in one place.

## Scope

**In:**
- `EvidenceClass` = `UEC0 | UEC1 | UEC2 | UEC3`
- `UncertaintyStatement` fields and invariants (see Design)
- `Uec0Reason` enumeration (`u_missing`, `model_missing`, `instrument_missing`) as a type (the rules that pick one are in `UNC.UEC.CLASS`, the suggested fixes in `UNC.UEC.EXPL`)
- Canonical JSON form (RFC 8785) with f64 values as IEEE-754 hex strings, and the statement digest

**Out:**
- Choosing the class (owned by `UNC.UEC.CLASS`)
- Commands that declare, show or explain statements (owned by `UNC.UEC.DECL` and `UNC.UEC.EXPL`)
- QIF 3.0 mapping (owned by `UNC.UEC.QIF`)
- Computing U, contributors or sensitivities (owned by `UNC.PROP`)

## Requirements

- **REQ-UNC.UEC.STMT-001** The `UncertaintyStatement` type shall hold exactly one `EvidenceClass` value, and no constructor shall produce a statement without one.
- **REQ-UNC.UEC.STMT-002** If a statement of class UEC-0 is constructed with any numeric uncertainty field set (U, k, standard uncertainty, interval bound, contributor value), then construction shall fail with error `uec0_has_value`.
- **REQ-UNC.UEC.STMT-003** If a statement of class UEC-1, UEC-2 or UEC-3 is constructed without a method tag, a unit, a model reference (ID, semver and SHA-256 digest), and either a coverage factor k or a coverage interval with its coverage probability, then construction shall fail with error `statement_incomplete` naming the first missing field.
- **REQ-UNC.UEC.STMT-004** The method tag shall be one of `LPU`, `MCM`, `ISO15530-3`, `ISO15530-4`, `DECLARED`, `SPEC_DERIVED`, and any other value shall be rejected with error `unknown_method`.
- **REQ-UNC.UEC.STMT-005** The canonical serialization of a statement shall be RFC 8785 JSON with every f64 written as its 16-digit IEEE-754 hexadecimal bit pattern, and its SHA-256 digest shall be identical on every platform for the same statement.
- **REQ-UNC.UEC.STMT-006** If a numeric field is NaN, infinite, or negative where the quantity is non-negative (U, k, standard uncertainty, coverage probability outside (0, 1)), then construction shall fail with error `invalid_number`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/unc/stmt/valid/*.json` (one statement per class and method), when each is parsed and re-serialized, then the output bytes equal the input bytes and the digest equals the value in `fixtures/unc/stmt/valid/digests.txt`.
- **AC-2** (error path) Given `fixtures/unc/stmt/invalid/uec0_with_U.json`, when parsed, then parsing fails with `uec0_has_value`.
- **AC-3** (error path) Given each file in `fixtures/unc/stmt/invalid/incomplete_*.json`, when parsed, then parsing fails with `statement_incomplete` and the named field matches the file suffix.
- **AC-4** (error path) Given `fixtures/unc/stmt/invalid/nan_U.json`, `neg_k.json` and `p_1.json`, when parsed, then each fails with `invalid_number`.
- **AC-5** (portability) Given AC-1's fixture, when the test runs on the wasm32 reference build and on x86-64 and ARM64 native builds, then all digests are identical.
- **AC-6** (property) Given 10,000 random valid statements from a seeded generator (seed `0x5EED0001`), when each is serialized and parsed, then the round trip is lossless.

## Design

- **Registry command(s):** none (types only).
- **Data structures:**
  - `enum EvidenceClass { Uec0, Uec1, Uec2, Uec3 }`
  - `enum Uec0Reason { UMissing, ModelMissing, InstrumentMissing }`
  - `enum Method { Lpu, Mcm, Iso15530_3, Iso15530_4, Declared, SpecDerived }`
  - `struct CoverageInterval { low: f64, high: f64, p: f64, kind: Shortest | ProbSymmetric }`
  - `struct ModelRef { id: String /* um:<name> */, semver, digest: Sha256 }`
  - `struct SourceRef { kind: Dcc | Qualification | TypeA | Declared | InstrumentModel, id, digest }`
  - `struct Contributor { input_id, sensitivity: Option<f64>, u_contrib: f64, variance_share: f64 }`
  - `struct UncertaintyStatement { class, uec0_reason: Option<Uec0Reason>, method: Option<Method>, unit: Option<UnitUri>, u_std: Option<f64>, expanded: Option<{ U: f64, k: f64 }>, interval: Option<CoverageInterval>, model: Option<ModelRef>, sources: Vec<SourceRef>, contributors: Vec<Contributor>, lpu_validated_by_mc: Option<bool>, nonlinearity: Option<NonlinearityOutcome>, selection_rule: Option<String>, estimate_convention: Option<String>, provenance: Provenance }`
  - Fields are private; only the validating constructor and the canonical parser create values. Vectors are kept in a stable order (sources by `id`, contributors by `input_id`).
- **Algorithm:** validation in a fixed order (class, then numbers, then completeness) so the first reported error is deterministic. Serialization uses the shared RFC 8785 canonicalizer from the kernel.
- **Errors:** `uec0_has_value`, `statement_incomplete{field}`, `unknown_method`, `invalid_number{field}`.
- **Determinism:** D0. No floating-point arithmetic; only bit-pattern serialization.
- **Performance budget:** serialize and digest 100,000 statements in under 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-kunc` (kernel side; no I/O; KNP-1 applies).
- **Pinned choices (build, value-affecting):**
  - Wire schema tag `uncertainty_statement@1` in member `schema`. Member names: `class`, `uec0_reason`, `method`, `unit`, `u_std`, `expanded {U, k}`, `interval {low, high, p, kind}`, `model {id, semver, digest}`, `sources [{kind, id, digest}]`, `contributors [{input_id, sensitivity, u_contrib, variance_share}]`, `lpu_validated_by_mc`, `nonlinearity {version, outcome, v1, v2, ratio, probed_inputs, probed_pairs, cross_terms_truncated, reason}`, `selection_rule`, `estimate_convention`, `provenance {rule_sets, principal, principal_kind, proposal, adopted_by, supersedes}`. Enum wire names: `UEC-0..UEC-3`; the six method tags; `shortest`, `prob_symmetric`; `dcc`, `qualification`, `type_a`, `declared`, `instrument_model`; `linear_adequate`, `second_order_needed`, `use_mcm`; `human`, `agent`.
  - Written with the kernel writer `fid_kci::kci::json` (strings, arrays, objects only). Every scalar is a string: f64 as exactly 16 lowercase hex digits of its bits (no prefix), booleans `"true"`/`"false"`, counts as plain decimal `u32`, digests `sha256:<64 hex>`. Absent optional members are omitted; `sources`, `contributors`, `provenance`, `provenance.proposal` and `provenance.rule_sets` are always written. Digest = SHA-256 (kernel `fid_kci`) of the canonical bytes, no trailing newline.
  - The parser accepts only the canonical bytes of a valid statement: anything else that is a valid statement fails `non_canonical`. Input at most 1 MiB, nesting at most 8, strings printable ASCII without escapes; other input fails `malformed{detail}`. Vectors at most 10 000 entries; text fields 1..256 printable ASCII bytes without space, `"` or `\`.
  - Numbers (bit tests and comparisons only): NaN, infinities and negative zero always fail. `u_std`, `U`, `k` strictly positive (a zero uncertainty is never valid; an absent one is UEC-0). `p` in the open interval (0, 1); `low < high` strictly; `u_contrib`, `v1`, `v2`, `ratio` non-negative; `variance_share` in [0, 1]; `sensitivity` any finite value.
  - Validation order: class (UEC-0 carries none of `method`, `u_std`, `expanded`, `interval`, non-empty `contributors`, `lpu_validated_by_mc`, `nonlinearity` -> `uec0_has_value{field}`; UEC-1..3 with a `uec0_reason` -> `class_mismatch`), numbers, completeness (UEC-0: `uec0_reason`; UEC-1..3: `method`, `unit`, `coverage`, `model`), text fields and unique IDs (`invalid_field{field}`), class consistency (`class_mismatch{field}`).
  - Class consistency (maintainer rule: claimed values never look measured): `DECLARED` and `SPEC_DERIVED` only with UEC-1; UEC-3 only with `ISO15530-3`/`ISO15530-4` (REQ-UNC.UEC-006); UEC-2 and UEC-3 need at least one source and only `dcc`, `qualification` or `type_a` sources (REQ-UNC.UEC-005). This is a type guard; `UNC.UEC.CLASS` still chooses the class. Provenance: `principal` and `principal_kind` set together; an agent principal or an `adopted_by` requires `proposal = true` (CONSTITUTION C5).
  - Unit URI: `<RFC 3986 scheme>:<non-empty rest>`. Model ID `um:<name>`; semver `MAJOR.MINOR.PATCH`, 1..9 digits each, no leading zeros. `sources`, `contributors` and `provenance.rule_sets` sorted by byte order of the ID; duplicates fail.
  - `Nonlinearity.lpu_reportable()` is derived from the outcome (`linear_adequate` only) rather than stored, so it cannot disagree with it.

## Test fixtures

Hand-written JSON statements under `fixtures/unc/stmt/` (CC-BY-4.0, authored in this project): one valid file per class and method, invalid files per error code, and a digests file written by the independent stdlib Python reference `fixtures/unc/stmt/make_fixtures.py` (checked with `--check` in CI) and verified by the tests on wasm32, x86-64 and ARM64. A seeded generator in the test module creates the property cases.

## Build prompt

> In crate `fid-kunc` (kernel layer, KNP-1 numeric profile: no FMA, no std HashMap or HashSet, NaN is an error, no panics, no `unwrap`/`expect`), add module `statement` with the types `EvidenceClass` (UEC-0..3), `Uec0Reason` (`u_missing`, `model_missing`, `instrument_missing`), `Method` (`LPU`, `MCM`, `ISO15530-3`, `ISO15530-4`, `DECLARED`, `SPEC_DERIVED`) and `UncertaintyStatement` with fields: class, optional UEC-0 reason, method, unit URI, standard uncertainty, expanded `{U, k}`, coverage interval `{low, high, p, kind}`, model reference `{id, semver, sha256}`, sources, contributors `{input_id, sensitivity, u_contrib, variance_share}`, `lpu_validated_by_mc`, nonlinearity outcome, selection-rule version, estimate convention and provenance. Fields are private; provide a validating constructor and a canonical parser only.
> Rules: UEC-0 must carry no numeric field (`uec0_has_value`); UEC-1..3 must carry method, unit, model ref and either k or an interval with p (`statement_incomplete{field}`, first missing field in declaration order); unknown method strings fail `unknown_method`; NaN, infinities, negative U/k/u, and p outside (0, 1) fail `invalid_number{field}`.
> Serialize as RFC 8785 canonical JSON with every f64 as a 16-hex-digit IEEE-754 bit string; digest = SHA-256 of those bytes. Keep sources and contributors sorted by ID.
> Tests: byte-exact round trip and digests for `fixtures/unc/stmt/valid/*.json`; each invalid fixture yields its error; 10,000 seeded random statements (seed `0x5EED0001`) round-trip; run the digest test on wasm32 and native targets. Never compute uncertainty in this module.

## Open questions

- Whether `kind` of coverage interval needs a third value for one-sided intervals is left to `UNC.PROP.MCM` [unverified].

## Children

_Leaf: none._
