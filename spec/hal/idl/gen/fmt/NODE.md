---
id: HAL.IDL.GEN.FMT
title: Capability-package description format (capability-package/v1)
level: 4
parent: HAL.IDL.GEN
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-hal-idl]
built_pr: 474
satisfies: [REQ-HAL.IDL.GEN-001]
covers: []
depends_on: [HAL.IDL.UNITS, HAL.IDL.BASE.ERR, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.GEN.FMT: Capability-package description format (capability-package/v1)

## Purpose

Settles the package format question left open by `HAL.IDL` (concept doc 08 H-D2). Capability packages are written as Rust types in the L0 crate `fid-hal-idl`; each package also produces a language-neutral **package description** in the project-owned format `capability-package/v1` (canonical JSON). Every generator, the discovery commands and the conformance suites read that description, never the Rust source. This avoids a proto3 or Arrow toolchain in the L0 crate while keeping one source of truth.

## Scope

**In:**
- The `capability-package/v1` JSON Schema: manifest (name, semantic version, deprecated flag, `tier3_usable` flag), operations (name, side-effect class, request and response type schemas, allowed error codes, live-changeable settings), batch schemas (ID, columns with required `EncodingDescriptor` and unit, validity mask, sequence range), semantics note (Markdown text in the project's own words and its digest), conformance profile (ID and items)
- The `CapabilityPackage` trait (`fn description() -> PackageDescription`) that every package implements
- A structural validator and the canonical digest (RFC 8785 + SHA-256)

**Out:**
- Package contents (owned by `HAL.IDL.BASE`, `HAL.IDL.CNC`, `HAL.IDL.SENS`)
- Writing description files and drift checks (owned by `HAL.IDL.GEN.EXPORT`)
- Host stubs (owned by `HAL.IDL.GEN.HOSTGEN`)

## Requirements

- **REQ-HAL.IDL.GEN.FMT-001** A `capability-package/v1` description shall hold, for one capability and version, the manifest, the operations with their side-effect classes and allowed error codes, the batch schemas, the semantics note and the conformance profile.
- **REQ-HAL.IDL.GEN.FMT-002** If a description has a duplicate operation name, a side-effect class outside CONSTITUTION §3, an error code outside the `Device@1` error model, a data-plane column without an `EncodingDescriptor`, or a version that is not semantic, then validation shall fail with `package_invalid` naming the JSON pointer.
- **REQ-HAL.IDL.GEN.FMT-003** The description digest shall be SHA-256 over RFC 8785 canonical JSON, identical on every supported platform, with operations, batch schemas and profile items sorted by name or ID.
- **REQ-HAL.IDL.GEN.FMT-004** Type schemas inside a description shall be JSON Schema 2020-12 fragments using only the keywords listed in the format's allowlist (`type`, `properties`, `required`, `items`, `enum`, `minimum`, `maximum`, `maxLength`, `$ref` within the description, `additionalProperties: false`).
- **REQ-HAL.IDL.GEN.FMT-005** A description shall contain no executable content and no reference outside itself other than digests of semantics notes and fixtures.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/idl/gen/fmt/demo_v1.json` (a two-operation demo package), when validated, then it passes and its digest equals `demo_v1.sha256` on Windows, Linux and macOS.
- **AC-2** (error path) Given `dup_op.json`, then validation fails with `package_invalid` at `/operations/1/name`.
- **AC-3** (error path) Given `bad_class.json` (class `Z9`), then it fails at the class pointer.
- **AC-4** (error path) Given `no_encoding.json` (a data-plane column without encoding), then it fails at that column.
- **AC-5** (error path) Given `foreign_keyword.json` (uses `patternProperties`), then it fails with `package_invalid` naming the keyword.
- **AC-6** Given `demo_v1.json` with operations permuted, then the canonical digest is unchanged.

## Design

- **Registry commands:** none; library.
- **Data structures:** `PackageDescription { manifest, operations: Vec<OperationDesc>, batch_schemas: Vec<BatchSchemaDesc>, semantics: SemanticsNote { text, digest }, profile: ConformanceProfileDesc }`, `trait CapabilityPackage`.
- **Format choice and alternatives:** project-owned JSON description exported from Rust (chosen); proto3 with custom options plus Arrow schemas (concept doc 08 H-D2; rejected for the L0 crate because it adds a code-generation toolchain and dependencies whose licences await D-001's allowlist); WIT as the source (rejected: covers only the Tier-3 subset). Arrow IPC can still be offered later as an export of batch schemas.
- **Errors:** `package_invalid { pointer, reason }`.
- **Determinism:** D0.
- **Performance budget:** validate a 50-operation description in under 5 ms [unverified until measured].
- **Crate:** `fid-hal-idl` (module `package`), layer L0.

## Test fixtures

Project-authored demo descriptions under `fixtures/hal/idl/gen/fmt/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `package`, define the `capability-package/v1` format: `PackageDescription { manifest { name, version (semver), deprecated, tier3_usable }, operations [{ name, class (CONSTITUTION §3 codes), request, response (JSON Schema 2020-12 fragments limited to the allowlist type/properties/required/items/enum/minimum/maximum/maxLength/internal $ref/additionalProperties:false), errors (ErrorCode subset) }], batch_schemas [{ id, columns [{ name, unit, encoding: EncodingDescriptor }] , validity_mask, seq_range }], semantics { text, digest }, profile { id, items } }`, the `CapabilityPackage` trait, `validate()` failing `package_invalid { pointer }` on duplicates, unknown classes or codes, missing encodings, non-semver versions and foreign schema keywords, and `digest()` = SHA-256 over RFC 8785 canonical JSON with lists sorted by name/ID (use the L0 canonical-JSON crate). No executable content. Publish the JSON Schema of the format at `schemas/hal/capability-package-v1.json`. No panic, unwrap, expect or unsafe; BTreeMap only (CONSTITUTION §8). Tests: AC-1..AC-6; AC-1 on all three OS runners.

## Open questions

- The format choice here departs from concept doc 08 H-D2 (proto3 plus Arrow); it is within the leaf's remit per `HAL.IDL` Open questions, but reviewers may prefer a decision record if third-party host authors object.

## Children

_Leaf: none._
