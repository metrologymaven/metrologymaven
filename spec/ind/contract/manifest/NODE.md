---
id: IND.CONTRACT.MANIFEST
title: Pack manifest schema and load-time validation
level: 3
parent: IND.CONTRACT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-001, REQ-IND.CONTRACT-003, REQ-IND.CONTRACT-004, REQ-IND.CONTRACT-005]
covers: []
depends_on: [ECO.SDK]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.CONTRACT.MANIFEST: Pack manifest schema and load-time validation

## Purpose

Defines the pack manifest: the machine-readable file in which a pack lists everything it registers, the standards and methods it implements, its maintainers and its reference datasets. Also owns the load-time checks that refuse a pack whose manifest is incomplete or whose registrations do not match it. It is the single source of truth every other `IND.CONTRACT` child reads.

## Scope

**In:**
- Manifest schema `fid.pack.manifest@1`, as an extension of the ECO.SDK plugin manifest (pack identity, SDK range and licence are inherited, not duplicated)
- Declared registrations: commands, element types, checks, report templates, exporters
- Standards declarations (designator plus edition) and method declarations (method ID plus semver)
- Maintainer list
- Reference-dataset references (dataset ID, path inside the package, SHA-256 digest, expected-results file)
- Load-time validation: unlisted registrations, missing editions, missing maintainers, missing datasets, malformed or oversized manifests

**Out** (and which node owns it instead):
- Which extension points a pack may use at all (owned by `IND.CONTRACT.PROFILE`)
- The expected-results file format and running the datasets (owned by `IND.CONTRACT.VERIFY`)
- Displaying the manifest to users (`fid pack show-pack`, owned by `IND.CONTRACT.LIFE`)
- Package format, signatures and the plugin loader (owned by `ECO.SDK`)

## Requirements

- **REQ-IND.CONTRACT.MANIFEST-001** The pack manifest schema shall be versioned (`fid.pack.manifest@<major>`) and shall list pack ID, pack version (semver), maintainers, declared standards and methods, commands, element types, checks, report templates, exporters and reference datasets.
- **REQ-IND.CONTRACT.MANIFEST-002** The pack manifest shall extend the ECO.SDK plugin manifest and shall not redefine its identity, SDK-range or licence fields.
- **REQ-IND.CONTRACT.MANIFEST-003** When a pack registers a command, element type, check, report template or exporter that its manifest does not list, the host shall refuse to load the pack and shall name the unlisted item.
- **REQ-IND.CONTRACT.MANIFEST-004** If a manifest declares a standard without an edition, then the host shall refuse to load the pack and shall name the standard.
- **REQ-IND.CONTRACT.MANIFEST-005** Every declared method shall carry a method ID of the form `<module>.<feature>.<method>@<semver>` (CONSTITUTION §3), and the host shall refuse a manifest with a malformed method ID.
- **REQ-IND.CONTRACT.MANIFEST-006** If a manifest names no maintainer, then the host shall refuse to load the pack.
- **REQ-IND.CONTRACT.MANIFEST-007** If a manifest references no reference dataset, or a dataset entry lacks a SHA-256 digest, then the host shall refuse to load the pack.
- **REQ-IND.CONTRACT.MANIFEST-008** The manifest parser shall be bounded (at most 1 MiB and 10 000 declared items [proposed limits]) and shall fail closed with a typed error on any violation.

## Interfaces

- **Offers:** the `PackManifest` type and schema file; `validate_manifest(manifest, registrations) -> Result<ValidatedPack, ManifestError>` used by the loader in `IND.CONTRACT.LIFE`; typed errors `E_PACK_UNLISTED`, `E_PACK_NO_EDITION`, `E_PACK_BAD_METHOD_ID`, `E_PACK_NO_MAINTAINER`, `E_PACK_NO_DATASET`, `E_PACK_MANIFEST_BOUNDS`.
- **Consumes:** ECO.SDK plugin manifest schema and package reader.

## Decomposition hints

SCHEMA (types and JSON schema), CHECK (load-time validation). Could become one leaf once ECO.SDK's manifest is fixed.

## Open questions

- The exact base manifest is not fixed until ECO.SDK is split; this node assumes a JSON manifest with an extension object.

## Children

_None yet: status `open`._
