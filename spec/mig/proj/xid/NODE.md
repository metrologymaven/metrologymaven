---
id: MIG.PROJ.XID
title: External-ID index and stable entity IDs
level: 3
parent: MIG.PROJ
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-002]
covers: []
depends_on: [IO.QIF.IDS, GDT.CHAR.IDENT, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PROJ.XID: External-ID index and stable entity IDs

## Purpose

Makes incumbent identity survive migration. Every entity imported by `MIG.PROJ` carries an `ExternalId` (incumbent namespace, record kind, incumbent identifier, source file digest), and its entity ID is a UUIDv5 derived from that, so importing the same file twice yields the same IDs and parallel-run pairing can find our characteristic from the incumbent's identifier. The index answers "which of our entities has this incumbent identifier?" for `MIG.PARALLEL.PAIR`.

## Scope

**In:**
- `ExternalId { namespace, kind: Feature | PointSet | Alignment | Characteristic | Datum, incumbent_id, source_sha256 }`
- UUIDv5 derivation under a fixed MIG namespace UUID from the canonical JSON of the `ExternalId`
- Attaching external IDs through the QIF identifier-preservation mechanism (`IO.QIF.IDS`) and, for characteristics, through `GDT.CHAR.IDENT` as an imported identity
- The `XidIndex`: lookup by (namespace, kind, incumbent_id) returning entity IDs across source files, and by entity ID returning its external IDs

**Out:**
- Pairing results (owned by `MIG.PARALLEL.PAIR`)
- Creating the entities (owned by `MIG.PROJ.POINTS`, `MIG.PROJ.ALIGN`, `MIG.PROJ.CHAR`)

## Requirements

- **REQ-MIG.PROJ.XID-001** Each imported entity's ID shall be the UUIDv5, under the fixed MIG namespace UUID, of the RFC 8785 canonical JSON of its `ExternalId`.
- **REQ-MIG.PROJ.XID-002** Each imported entity shall store its `ExternalId` so that it survives QIF export and re-import unchanged.
- **REQ-MIG.PROJ.XID-003** When the index is queried by namespace, kind and incumbent identifier, it shall return every matching entity ID sorted by source digest, and an empty list when none exists.
- **REQ-MIG.PROJ.XID-004** If an incumbent identifier is empty or longer than 256 bytes, then derivation shall fail with `xid_invalid` and the item shall be reported as unmappable by the caller.
- **REQ-MIG.PROJ.XID-005** Incumbent identifiers shall be stored as untrusted text and compared byte-exactly after Unicode NFC normalisation.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/proj/xid/ids.json` (20 external IDs with expected UUIDs), when derived, then every UUID matches.
- **AC-2** Given a project where the same file was imported twice, then the index returns one entity ID per incumbent identifier.
- **AC-3** Given two different source files with the same incumbent identifier, then the index returns two entity IDs sorted by source digest.
- **AC-4** Given an entity exported to QIF and re-imported (fixture `roundtrip.qif`), then its `ExternalId` is unchanged.
- **AC-5** (error path) Given an empty identifier, then derivation fails with `xid_invalid`.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `ExternalId`; `XidIndex` built from the document's entities (BTreeMap keyed by `(namespace, kind, incumbent_id)`).
- **Algorithm:** NFC-normalise identifier → bounds check → canonical JSON (`CORE.JRN.CANON`) → UUIDv5 (SHA-1 per RFC 9562 [unverified RFC number for the UUID revision]) with namespace constant `fid-mig-xid`.
- **Errors:** `xid_invalid`.
- **Determinism:** D0.
- **Performance budget:** index of 100,000 entities built in under 100 ms [unverified until measured].
- **Crate:** `fid-mig` (module `proj::xid`).

## Test fixtures

`fixtures/mig/proj/xid/`: generated identifier sets and a small QIF round-trip file written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::xid` implementing spec node MIG.PROJ.XID: `ExternalId { namespace, kind, incumbent_id, source_sha256 }`; entity ID = UUIDv5 under a fixed MIG namespace UUID of the RFC 8785 canonical JSON of the ExternalId; identifiers NFC-normalised, untrusted, 1..256 bytes (`xid_invalid` otherwise); attach ExternalIds through the QIF identifier-preservation pass-through so they survive export and re-import; `XidIndex` lookups by (namespace, kind, incumbent_id) returning entity IDs sorted by source digest, and reverse lookups. Tests for AC-1..AC-5. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
