---
id: IO.CLOUD.E57.CVEC
title: E57 compressed-vector sections and bit-pack codec
level: 4
parent: IO.CLOUD.E57
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CLOUD.E57-002, REQ-IO.CLOUD.E57-005]
covers: []
depends_on: [IO.CLOUD.E57.PHYS, IO.CLOUD.E57.XML]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CLOUD.E57.CVEC: Compressed-vector sections and bit-pack codec

## Purpose

Reads and writes the binary compressed-vector sections that hold E57 point records: the section header, data packets carrying one bytestream per field, index and empty packets, and the bit-pack codec for integers, scaled integers and floats. It decodes records field by field in record order with bounded buffers, keeping f64 values exact and reporting each field's source precision.

## Scope

**In:**
- Compressed-vector section header and packet types (data, index, empty) with length checks
- Bit-pack decoding and encoding: integers with bit width from the declared min/max, scaled integers (raw integer plus scale and offset), 32-bit and 64-bit floats
- Record assembly across packets for all prototype fields, yielding batches of up to 65,536 records
- Source precision per field (`f64`, `f32`, or scaled integer with its scale) for provenance
- Encoding of f64 Cartesian fields and selected attributes for the writer

**Out:**
- Page checksums (`IO.CLOUD.E57.PHYS`), prototypes (`IO.CLOUD.E57.XML`)
- Mapping fields to point attributes (`IO.CLOUD.E57.READ`)

## Requirements

- **REQ-IO.CLOUD.E57.CVEC-001** Decoded 64-bit float fields shall equal the stored IEEE-754 values bit for bit.
- **REQ-IO.CLOUD.E57.CVEC-002** Scaled-integer fields shall be decoded as offset + scale × raw in f64 with one rounding, and the field's scale shall be reported as its source precision.
- **REQ-IO.CLOUD.E57.CVEC-003** If a packet length, bytestream length or decoded record count disagrees with the section header or prototype, then decoding shall fail with `malformed` naming the section offset and packet index.
- **REQ-IO.CLOUD.E57.CVEC-004** If a decoded integer lies outside the prototype's declared min/max, then decoding shall fail with `malformed{reason: value_out_of_range}` naming the field and record index.
- **REQ-IO.CLOUD.E57.CVEC-005** Records encoded by the writer and decoded by the reader shall be bit-identical for every field kind.
- **REQ-IO.CLOUD.E57.CVEC-006** Decoder buffers shall stay within a fixed budget (default 64 MiB) independent of section size.

## Acceptance criteria

- **AC-1** Given 10^6 seeded records with f64 x,y,z, f32 intensity and u8 colour, when encoded and decoded, then all fields are bit-identical.
- **AC-2** Given a scaled-integer field (scale 1e-4, offset 0) with raw values ±2^20, then decoded values equal offset + scale × raw computed with one rounding, and precision reports scale 1e-4.
- **AC-3** (error path) Given `fixtures/io/cloud/e57/cvec/short_packet.bin`, then `malformed{packet: 3}`.
- **AC-4** (error path) Given an integer field with a value above its declared max, then `malformed{reason: value_out_of_range}`.
- **AC-5** Given a 10^8-record seeded section, then peak decoder buffer memory stays under 64 MiB [unverified until measured].
- **AC-6** Given the CVEC fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `CvecReader { next_batch() -> Result<Option<RecordBatch>> }`; `CvecWriter { push(batch), finish() -> SectionRef }`; `RecordBatch { fields: Vec<FieldColumn> }`; `FieldColumn::{F64, F32, I64, Scaled{raw: Vec<i64>, scale, offset}}`; `Precision`.
- **Algorithm:** per-field bitstream cursors; refill from the next data packet when a cursor runs dry; bit width = ceil(log2(max − min + 1)); KNP-1 arithmetic for scaled values.
- **Errors:** `malformed{offset, packet?, reason?}`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** ≥ 30 M records/s decode for 3×f64 [unverified until measured].
- **Crate:** `fid-io-e57`.

## Test fixtures

Seeded encode/decode tests and hand-built hostile section files under `fixtures/io/cloud/e57/cvec/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-e57`, implement compressed-vector sections: parse the section header and data/index/empty packets with strict length checks (`malformed{offset, packet}`), decode bit-packed integer (width from declared min/max), scaled-integer (offset + scale × raw, one rounding, report scale as precision) and 32/64-bit float bytestreams into record batches of ≤ 65,536, reject out-of-range integers (`malformed{reason: value_out_of_range}`), keep decoder buffers under 64 MiB; implement the matching encoder so encode→decode is bit-identical. Confirm packet layout against ASTM E2807 by designator only. KNP-1 arithmetic; CONSTITUTION §8: no panic, no unwrap. Fuzz target. Tests AC-1..AC-6.

## Open questions

- Packet layout details to confirm against ASTM E2807 [unverified].

## Children

_Leaf: none._
