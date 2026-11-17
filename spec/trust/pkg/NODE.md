---
id: TRUST.PKG
title: Result Package format
level: 2
parent: TRUST
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-001, REQ-TRUST-003]
covers: []
depends_on: [CORE, IO, UNC]
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
- **Consumes:** ResultRecords and journal segments from `CORE`; QIF Results serialisation from `IO`; UEC status from `UNC`; kernel manifest from `KRN`.

## Decomposition hints

MANIFEST (schema + versioning), EXPORT (assembler command), READER (bounded reader + limits), FIXTURES (golden packages, malformed-package corpus).

## Open questions

- Container choice (ZIP, tar or directory + manifest) is a leaf design choice; it must be readable by common tools without fid.
- Whether the package export is X1 (egress) or a W-class write to an export root needs a ruling consistent with AUTO's export-root model.
- The in-toto predicate type URI needs a stable home, which depends on D-002 (project name).

## Children

_None yet: status `open`._
