---
id: TRUST.PKG
title: Result Package format
level: 2
parent: TRUST
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-001, REQ-TRUST-003]
covers: []
depends_on: [CORE, IO]
cli: [fid sign export-package, fid verify inspect-package]
mcp: [verify_inspect_package]
journal: [sign.export_package@1]
updated: 2026-11-16
---

# TRUST.PKG: Result Package format

## Purpose

Defines the open, versioned **Result Package**: the self-contained bundle that carries exported results, their provenance and (optionally) their input points, so that anyone holding only the package and `fid-verify` can check it. This node owns the package layout, its manifest, its canonical digests and the export command that assembles an unsigned package. It exists separately because the format is a public contract (C8) consumed by signing, verification and replay, and must stay stable across all of them.

## Scope

**In:**
- Package container layout and manifest schema, with a format version
- Content inventory: ResultRecords (CONSTITUTION §5), QIF Results payload, journal segment and checkpoint(s), kernel manifest reference, algorithm IDs, input digests
- Optional evidence tiers: results only, features, or full points (matches `evaluation_basis`)
- Canonical serialisation and per-member digests (RFC 8785 + SHA-256, as in CONSTITUTION §6)
- The in-toto Statement / predicate type that the DSSE payload carries
- Size and member-count limits; fail-closed reading rules for the format
- `sign.export_package@1` (assembles an unsigned package into an approved export root)
- Published format specification and conformance fixtures

**Out** (and which node owns it instead):
- Producing and checking DSSE signatures (`TRUST.SIGN`)
- Human signature meanings recorded in the package (`TRUST.ESIG`)
- Journal hash-chain and checkpoint semantics (`TRUST.CHAIN`, built on `CORE`)
- Recomputing results from included points (`TRUST.REPLAY`)
- QIF readers and writers themselves (`IO`)

## Requirements

- **REQ-TRUST.PKG-001** The Result Package specification shall be published under the project's open spec licence and carry a semantic format version in every package manifest.
- **REQ-TRUST.PKG-002** The package manifest shall list every member with its media type, byte length and SHA-256 digest, and the DSSE payload shall be an in-toto Statement whose subjects are those members [unverified: predicate type URI to be registered].
- **REQ-TRUST.PKG-003** Every ResultRecord in a package shall carry all minimum fields of CONSTITUTION §5, and an export shall fail with a named error if any field is missing.
- **REQ-TRUST.PKG-004** When the exporter is asked to include points, the package shall contain every input point set that the included results were computed from, referenced by the input digests in each ResultRecord.
- **REQ-TRUST.PKG-005** The package shall contain the journal segment that produced its results, from the nearest checkpoint to the last included entry, so that the chain can be checked without the original project.
- **REQ-TRUST.PKG-006** If an export would include a result with uncertainty class UEC-0, then the package shall record it as "not evaluated" and shall never substitute a value.
- **REQ-TRUST.PKG-007** A conforming package reader shall reject, with a typed error and without panicking, any package that exceeds the published limits (initial targets: 16 GiB total, 1,000,000 members, 4 KiB path length) [unverified].
- **REQ-TRUST.PKG-008** Serialising the same project state twice shall produce byte-identical manifests and member digests (determinism D0).
- **REQ-TRUST.PKG-009** The exporter shall write only into an approved export root handle and shall never accept a raw file path from an agent principal.

## Interfaces

- **Offers:** the Result Package specification and its JSON Schema; `sign.export_package@1` (CLI `fid sign export-package`); a bounded package reader library used by `TRUST.SIGN`, `TRUST.CHAIN`, `TRUST.REPLAY` and `TRUST.VERIFY`; `verify.inspect_package` (R0) listing a package's contents.
- **Consumes:** ResultRecords (including their UEC status, defined by UNC) and journal segments from `CORE`; QIF Results serialisation from `IO`; kernel manifest from `KRN`.

## Decomposition hints

MANIFEST (schema + versioning), EXPORT (assembler command), READER (bounded reader + limits), FIXTURES (golden packages, malformed-package corpus).

## Split rationale

The split follows **the package's life from definition to consumption**: the byte format and statement (MANIFEST), deciding what goes in (CONTENT), writing it out (EXPORT), reading it back safely (READER), and the public conformance set (FIXTURE). Only CONTENT reaches into CORE, IO and KRN, so the format, writer and reader stay small and dependency-light for `fid-verify`.

Alternatives considered:
- **Writer vs reader halves only.** Rejected: content selection rules and the format contract change for different reasons, and the reader is the fuzzing hot spot.
- **Fixtures inside each leaf.** Rejected: third-party verifiers need one published conformance set with its own spec text.

## Open questions

- Container choice is settled in `TRUST.PKG.MANIFEST`: ZIP with zip64 and deterministic metadata, readable with common tools.
- The export class is settled by D-007 / CONSTITUTION §3.1: W1 with `fs_effect: write_root` for a local export root; X1 only off-machine (`TRUST.PKG.EXPORT`).
- The in-toto predicate type URI needs a stable home, which depends on D-002 (project name); one crate constant holds it.
- `depends_on` no longer lists UNC: the UEC status the package carries is a ResultRecord field (CONSTITUTION §5) read through CORE. The edge closed a dependency cycle with `GDT.RES.ROWFLOW -> TRUST.ESIG -> TRUST.PKG -> UNC -> UNC.ORS -> GDT.RES`.
- The MCP tool `sign_export_package` falls outside TRUST's `verify_*` MCP pattern (already raised in TRUST).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.PKG.MANIFEST | Package container layout, manifest and in-toto statement | 001 002 008 | none |
| TRUST.PKG.CONTENT | Package content selection and evidence tiers | 003 004 005 006 | none |
| TRUST.PKG.EXPORT | Export an unsigned Result Package to an export root | 008 009 | none |
| TRUST.PKG.READER | Bounded package reader and inspect command | 007 | none |
| TRUST.PKG.FIXTURE | Published package spec and conformance fixtures | 001 007 008 | none |
