---
id: HAL.IDL.NEG
title: Capability versioning and negotiation rules
level: 3
parent: HAL.IDL
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-hal-idl]
satisfies: [REQ-HAL.IDL-002, REQ-HAL.IDL-007]
covers: []
depends_on: [HAL.IDL.BASE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IDL.NEG: Capability versioning and negotiation rules

## Purpose

The pure negotiation function that, at session open, compares what the runtime supports with a driver's `CapabilityDeclaration` and picks, per capability, a version and a feature set, or fails with `UNSUPPORTED`. It also encodes the semver rules every capability package follows. It is a separate leaf so the runtime (`HAL.ACQ.SESS`), the conformance suite and third-party SDKs all use one tested implementation.

## Scope

**In:**
- `CapabilityDeclaration` type (driver identity, device identity, per-capability version range, features, limits, time sources, tested firmware range and conformance-report digest)
- `RuntimeSupport` table: per capability, supported majors with their highest minor and known features, and a deprecation marker for the previous major
- `negotiate(support, declaration) -> NegotiationResult`
- Semver rules for capability packages

**Out:**
- Executing negotiation in a session and journaling it in `device.session_opened` (owned by `HAL.ACQ.SESS`)
- Package contents (owned by `HAL.IDL.BASE`, `HAL.IDL.CNC`, `HAL.IDL.SENS`)

## Requirements

- **REQ-HAL.IDL.NEG-001** For each capability in a declaration, `negotiate` shall select the highest major version supported by both sides and, within it, the highest minor version supported by both sides.
- **REQ-HAL.IDL.NEG-002** The negotiated feature set shall be the intersection of declared and runtime-known features, sorted by name; declared features unknown to the runtime shall be returned in a sorted `ignored_features` list.
- **REQ-HAL.IDL.NEG-003** If no common major version exists for a capability, then that capability shall be returned as `UNSUPPORTED`, and if `Device` itself is unsupported the whole negotiation shall fail with `UNSUPPORTED`.
- **REQ-HAL.IDL.NEG-004** The runtime-support table shall keep the previous major version of every capability negotiable, marked deprecated, for at least one release after a new major is introduced.
- **REQ-HAL.IDL.NEG-005** Given identical inputs, `negotiate` shall return byte-identical canonical JSON output.
- **REQ-HAL.IDL.NEG-006** If a declaration contains a malformed version range or duplicates a capability name, then `negotiate` shall fail with `declaration_invalid` naming the entry.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/neg/support_v1.json` and `fixtures/hal/idl/neg/decl_gocator.json` (Device >=1.0 <2, LineScan >=1.2 <2 with features intensity, encoder_trigger, raw_profiles), when `negotiate` runs, then the result is Device 1.x highest common minor and LineScan 1.3 with the expected feature intersection in `expected_gocator.json`.
- **AC-2** Given `decl_future.json` (LineScan >=2.0 <3 only) against `support_v1.json`, when `negotiate` runs, then LineScan is `UNSUPPORTED` and Device succeeds.
- **AC-3** (error path) Given `decl_no_device.json` (no compatible Device major), when `negotiate` runs, then it fails with `UNSUPPORTED`.
- **AC-4** Given `support_v2_with_v1_deprecated.json` and a declaration offering only LineScan 1.x, when `negotiate` runs, then LineScan 1.x is selected and flagged `deprecated: true`.
- **AC-5** (error path) Given `decl_duplicate.json`, when `negotiate` runs, then it fails with `declaration_invalid` naming `LineScan`.
- **AC-6** Given AC-1 inputs, when `negotiate` runs 1 000 times with declaration capabilities permuted by a seeded shuffle, then every output is byte-identical.

## Design

- **Registry commands:** none; called by `device.open_session@1` in `HAL.ACQ.SESS`.
- **Data structures:** `CapabilityDeclaration`, `CapabilityOffer { name, versions: VersionRange, features: Vec<String>, limits: BTreeMap<String, Quantity> }`, `RuntimeSupport`, `NegotiationResult { per_capability: BTreeMap<String, Negotiated | Unsupported>, ignored_features }`.
- **Algorithm:** parse ranges (`>=a.b <c`), intersect with support per major descending, take max minor; set operations over sorted vectors.
- **Errors:** `UNSUPPORTED`, `declaration_invalid`.
- **Determinism:** D0; output sorted by capability then feature name.
- **Performance budget:** < 100 µs for 12 capabilities [unverified until measured].
- **Crate:** `fid-hal-idl` (module `negotiate`).

## Test fixtures

Project-authored JSON declarations and support tables under `fixtures/hal/idl/neg/` (CC-BY-4.0). The Gocator-style declaration uses invented serials and is illustrative only.

## Build prompt

> In crate `fid-hal-idl`, module `negotiate`, implement `negotiate(support: &RuntimeSupport, decl: &CapabilityDeclaration) -> Result<NegotiationResult, NegError>`. Rules: per capability choose the highest common major, then highest common minor; features = sorted intersection, with declared-but-unknown features in a sorted `ignored_features`; no common major → that capability `UNSUPPORTED`; if `Device` is unsupported the call fails `UNSUPPORTED`; malformed ranges or duplicate capability names fail `declaration_invalid`; a previous major kept in the support table is selectable and flagged `deprecated`. Device strings in the declaration are `UntrustedText`. Output must be canonical-JSON byte-identical for permuted inputs. BTreeMap only, no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-6 with fixtures in `fixtures/hal/idl/neg/`.

## Open questions

- Whether a driver may decline a negotiated feature after open (for example when a licence option is missing) is left to `HAL.ACQ.SESS`; this leaf assumes declarations are truthful at open.

## Children

_Leaf: none._
