---
id: MIG.PROJ.MAPREP
title: Mapping report model and classification
level: 3
parent: MIG.PROJ
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-MIG.PROJ-001]
covers: []
depends_on: [MIG.PROJ.PROFILE, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.PROJ.MAPREP: Mapping report model and classification

## Purpose

Every import says honestly what it did with each source item. The mapping report (`mig-mapreport@1`) lists every feature, point set, alignment, characteristic and datum reference found in the source, with the taxonomy construct it is, the entity it became, and a status of `MAPPED`, `PARTIAL` or `UNSUPPORTED` with a typed reason. The report is attached to the import's journal event and is the input that `MIG.COVER.GEN` counts across a corpus.

## Scope

**In:**
- The `mig-mapreport@1` schema: header (profile ID, version and digest; source file digest; format ID; importer version), items (source locator, construct ID, status, reason code, entity ID if any, detail as untrusted text)
- The closed reason-code list (e.g. `unsupported_construct`, `missing_field`, `unit_unknown`, `frame_unknown`, `dialect_unset`, `datum_unresolved`, `xid_invalid`, `value_out_of_range`)
- Builder API used by the importers, with sorting and digest
- A summary (counts per status and per construct)

**Out:**
- Deciding a status for a given record (done by `MIG.PROJ.POINTS`, `MIG.PROJ.ALIGN`, `MIG.PROJ.CHAR` through this builder)
- Aggregation across a corpus (owned by `MIG.COVER.GEN`)

## Requirements

- **REQ-MIG.PROJ.MAPREP-001** The report shall contain exactly one item for every source record that the profile recognises as a feature, point set, alignment, characteristic or datum reference, and one `UNSUPPORTED` item with reason `unsupported_construct` for every source record the profile does not map.
- **REQ-MIG.PROJ.MAPREP-002** Every `PARTIAL` or `UNSUPPORTED` item shall carry a reason code from the closed list, and a `MAPPED` item shall carry no reason code.
- **REQ-MIG.PROJ.MAPREP-003** Every item shall carry the construct ID from the profile and, if an entity was created, its entity ID.
- **REQ-MIG.PROJ.MAPREP-004** The report shall be sorted by source locator and its digest shall be the SHA-256 of its RFC 8785 canonical JSON.
- **REQ-MIG.PROJ.MAPREP-005** If the builder is finished with an item whose status and reason violate REQ-002, then it shall fail with `maprep_invalid` and produce no report.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/proj/maprep/items.json` (30 builder calls in random order), when the report is finished, then it equals `expected.json` byte for byte, with summary counts 22 `MAPPED`, 5 `PARTIAL`, 3 `UNSUPPORTED`.
- **AC-2** (error path) Given a `MAPPED` item with reason `missing_field`, then finishing fails with `maprep_invalid`.
- **AC-3** Given AC-1 with the calls in another order, then the digest is unchanged.

## Design

- **Registry command(s):** none; library used by the import commands, which embed the report in their result and journal its digest.
- **Data structures:** `MappingReport { schema: "mig-mapreport@1", header, items: Vec<MapItem>, summary }`, `MapItem { locator, construct_id, status, reason?, entity?, detail?: UntrustedText }`, `enum Reason` (closed).
- **Algorithm:** collect → validate → sort by locator (line/row for text formats, element path for QIF) → summary → canonical JSON → SHA-256.
- **Errors:** `maprep_invalid`.
- **Determinism:** D0.
- **Performance budget:** 100,000 items finished in under 200 ms [unverified until measured].
- **Crate:** `fid-mig` (module `proj::maprep`).

## Test fixtures

`fixtures/mig/proj/maprep/`: builder call scripts and expected reports written for the project, CC0-1.0.

## Build prompt

> In crate `fid-mig`, add module `proj::maprep` implementing spec node MIG.PROJ.MAPREP: the `mig-mapreport@1` types (header with profile ID/version/digest, source digest, format ID, importer version; items with locator, construct ID, status `MAPPED|PARTIAL|UNSUPPORTED`, closed reason enum, optional entity ID, untrusted detail), a builder that rejects `MAPPED` items with a reason or non-mapped items without one (`maprep_invalid`), sorts by locator, computes per-status and per-construct summary counts and the SHA-256 of RFC 8785 canonical JSON. Tests for AC-1..AC-3. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- None.

## Children

_Leaf: none._
