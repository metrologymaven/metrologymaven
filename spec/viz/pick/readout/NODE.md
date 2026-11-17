---
id: VIZ.PICK.READOUT
title: Hover readouts
level: 3
parent: VIZ.PICK
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK-006]
covers: []
depends_on: [VIZ.PICK.RESOLVE, GEO.FRAME, KRN.NUM]
side_effect: R0
determinism: D0
cli: [fid view get-readout]
mcp: [view_get_readout]
journal: [view.get_readout@1]
updated: 2026-11-16
---

# VIZ.PICK.READOUT: Hover readouts

## Purpose

Show what is under the pointer: entity name and type, coordinates in the active coordinate frame, and the deviation value of record where one exists, all from CPU f64 data formatted by the kernel formatter with a provenance badge. The same readout is available as an R0 command, so an agent can ask "what is at this hit" and get the same text.

## Scope

**In:** readout assembly for a `PickHit`, coordinate expression in the active frame, deviation lookup by point or vertex index, debounce on pointer rest, `view.get_readout@1`.
**Out:** the hit itself (`VIZ.PICK.RESOLVE`); computing deviations (`SCAN.CMP`); number formatting (`KRN.NUM`); tooltip widget styling (`VIZ.SHELL`).

## Requirements

- **REQ-VIZ.PICK.READOUT-001** A readout shall contain the entity name, type, coordinates in the active frame and, where the entity has a deviation field, the deviation value of record at the hit point or nearest vertex, each with a provenance badge.
- **REQ-VIZ.PICK.READOUT-002** Every number in a readout shall be computed from f64 CPU data and formatted by the kernel formatter, and the readout shall name the formatter ID.
- **REQ-VIZ.PICK.READOUT-003** When the pointer rests, the readout shall update within 50 ms on the reference profile [unverified].
- **REQ-VIZ.PICK.READOUT-004** When the deviation shown is that of the nearest vertex rather than the exact hit point, the readout shall say so.
- **REQ-VIZ.PICK.READOUT-005** Entity names from imported content shall be marked as untrusted text and shown as plain text.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/devfield-10k` and a hit on point index 4321, when `fid view get-readout --entity <cloud-id> --index 4321 --json` runs, then the deviation string equals the kernel formatter's output for that point's f64 deviation, and the formatter ID is present.
- **AC-2** Given an active alignment frame `A1` in the fixture, then the coordinates equal the kernel transform of the point into `A1`, formatted by the kernel formatter.
- **AC-3** Given a mesh hit inside a triangle, then the readout states "nearest vertex" and shows that vertex's value of record.
- **AC-4** (error path) Given an unknown entity ID, when the command runs, then it fails with `UnknownEntity` and returns no readout.
- **AC-5** Given an entity whose name contains markup characters, then the readout returns it in an `untrusted_text` field unchanged.

## Design

- **Registry command(s):** `view.get_readout@1`, R0. Parameters: `entity: EntityId`, `index: u32?`, `point_m: [f64; 3]?`, `frame: FrameId?` (default active). Result: `Readout { name: UntrustedText, kind, coords: [FormattedValue; 3], deviation: FormattedValue?, nearest_vertex: bool, formatter_id }`.
- **Algorithm:** look up the entity; transform the f64 point into the frame through GEO; fetch the deviation value of record by index (or nearest vertex index via GEO BVH); format through `KRN.NUM`; attach provenance.
- **Errors:** `UnknownEntity`, `UnknownFrame`, `IndexOutOfRange`.
- **Determinism:** D0 (pure f64 lookups and kernel formatting).
- **Performance budget:** command under 5 ms; UI debounce 30 ms [unverified].
- **Crate:** `fid-pick` (module `readout`).

## Test fixtures

`fixtures/viz/devfield-10k` (shared with VIZ.OVERLAY.LEGEND) plus a generated alignment frame `A1`; in-repo.

## Build prompt

> In crate `fid-pick`, add module `readout` and registry command `view.get_readout@1` (R0; CLI `fid view get-readout`, MCP `view_get_readout`). Given an entity ID and a point index or f64 point, return name (as untrusted text), type, coordinates transformed into the active or requested frame by GEO's f64 transforms, and the deviation value of record at that index or the nearest vertex (flag `nearest_vertex`), all formatted by the kernel formatter with its ID and a provenance badge. Never use f32 display data. In the GUI, show the readout after a 30 ms pointer rest. Tests: deviation string equals kernel formatter output; coordinates in frame A1; nearest-vertex flag on mesh; `UnknownEntity` error; untrusted name returned verbatim. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; untrusted text is data (CONSTITUTION C6).

## Open questions

- None blocking.

## Children

_Leaf: none._
