---
id: IO.CLOUD.STREAM
title: Streaming point sink and attribute selection for cloud readers and writers
level: 3
parent: IO.CLOUD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD-007, REQ-IO.CLOUD-008]
covers: []
depends_on: [IO.REG.ENV, GEO.OOC]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.CLOUD.STREAM: Streaming point sink and attribute selection

## Purpose

The shared plumbing between cloud readers and GEO's out-of-core chunked clouds: a `PointSink` that readers push batches into, which writes chunks to staged blobs with bounded resident memory, and a `PointSource` with an attribute-selection model that exporters pull from. Without it, every format would re-solve memory bounding and attribute mapping.

## Scope

**In:**
- `PointSink` (batch push, per-scan grouping, attribute columns, staged blobs committed only on success)
- `PointSource` (chunked read for exporters) and `AttributeSelection` (which per-point attributes to write, with type mapping and a report of attributes that the target format cannot hold)
- Resident-memory accounting against `IO.REG.LIMITS`
- An in-memory sink for small files and tests

**Out:**
- Chunk layout, LOD and octrees (GEO.OOC)
- Format parsing and writing (format siblings)

## Requirements

- **REQ-IO.CLOUD.STREAM-001** While importing, the sink shall keep resident memory for point data at or below a configurable cap (default 1 GiB) independent of file size, so that a 10^8-point import stays under 4 GiB peak resident memory [unverified until measured].
- **REQ-IO.CLOUD.STREAM-002** When an import fails or is cancelled, every chunk the sink staged shall be unreachable from the document and eligible for garbage collection.
- **REQ-IO.CLOUD.STREAM-003** When an export selects attributes, the source shall deliver exactly the selected attributes in the requested order, and any selected attribute the target format cannot represent shall fail the export with `attribute_unsupported` naming it.
- **REQ-IO.CLOUD.STREAM-004** Points pushed in one batch order shall be stored and later read back in the same order.

## Interfaces

- **Offers:** `PointSink`, `PointSource`, `AttributeSelection`, `AttributeType` mapping table used by `IO.CLOUD.ASCII`, `IO.CLOUD.E57`, `IO.CLOUD.LAS`, `IO.CLOUD.PTX`.
- **Consumes:** GEO.OOC chunked clouds; CORE.STO staged blobs (through `IO.REG.ENV`).

## Decomposition hints

SINK (staging and memory cap), SOURCE (chunked pull), ATTR (attribute selection and type mapping).

## Open questions

- Depends on the CORE.STO staged-blob lifecycle noted in `IO.REG.ENV`.

## Children

_None yet: status `open`._
