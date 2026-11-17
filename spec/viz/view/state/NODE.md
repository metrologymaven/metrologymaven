---
id: VIZ.VIEW.STATE
title: ViewState model and canonical form
level: 3
parent: VIZ.VIEW
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.VIEW-001, REQ-VIZ.VIEW-002]
covers: []
depends_on: [CORE.JRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.VIEW.STATE: ViewState model and canonical form

## Purpose

The single serialisable description of what a viewport shows: f64 camera pose and projection, clip planes, visibility, transparency, references to the active ColorMapSpec, legend layout and LabelSet, and the viewport layout. Its canonical serialisation (RFC 8785 JSON) and SHA-256 digest are what NamedViews store, reports reference and replay compares byte for byte.

## Scope

**In:** the `ViewState` types, validation, canonical serialisation and parsing, digests, and schema versioning.
**Out:** commands that change view state (`VIZ.VIEW.CAMERA`, `VIZ.VIEW.CLIP`, `VIZ.VIEW.VISIB`, `VIZ.VIEW.TRANSP`); NamedView lifecycle (`VIZ.VIEW.NAMED`); multi-viewport layouts (`VIZ.VIEW.MULTI`, which embeds per-viewport ViewStates); colour-map and label content (`VIZ.OVERLAY`).

## Requirements

- **REQ-VIZ.VIEW.STATE-001** `ViewState` shall hold camera position, orientation and target as f64 in metres, projection (perspective field of view or orthographic height), clip planes, visibility rules, transparency entries, ColorMapSpec reference, legend layout reference and LabelSet reference.
- **REQ-VIZ.VIEW.STATE-002** The canonical serialisation of a ViewState shall be RFC 8785 JSON, and parsing a canonical form and re-serialising it shall reproduce identical bytes.
- **REQ-VIZ.VIEW.STATE-003** f64 values shall serialise by the shortest round-trip representation so that parsing returns the identical f64 bit pattern.
- **REQ-VIZ.VIEW.STATE-004** Collections in a ViewState (clip planes, visibility rules, transparency entries) shall serialise in a defined order independent of insertion order.
- **REQ-VIZ.VIEW.STATE-005** If a serialised ViewState is malformed, over size limits (more than 64 clip planes or 100 000 entries) or contains a non-finite number, then parsing shall fail with a typed error.
- **REQ-VIZ.VIEW.STATE-006** Every ViewState shall carry a schema version, and the digest shall be the SHA-256 of the canonical bytes.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/viewstate-samples` (20 hand-written ViewStates), when each is parsed and re-serialised, then the output bytes equal the input bytes.
- **AC-2** (property) Given 10 000 generated ViewStates (seed 3) with random f64 values including subnormals and ±0, when serialised and parsed, then every f64 has the identical bit pattern.
- **AC-3** Given two ViewStates built with the same clip planes inserted in different orders, then their canonical bytes and digests are identical.
- **AC-4** (error path) Given a ViewState with a NaN camera coordinate, when parsed, then it fails with `NonFiniteNumber`; given 65 clip planes, it fails with `LimitExceeded`.
- **AC-5** Given a ViewState of schema version 1, then its digest matches the stored golden digest in the fixture.

## Design

- **Registry command(s):** none (data model).
- **Data structures:** `ViewState { schema: u16, camera: Camera { eye_m, target_m, up: [f64; 3], projection: Perspective { fov_y_rad } | Ortho { height_m } }, clips: Vec<ClipPlane>, visibility: Vec<VisibilityRule>, transparency: Vec<TransparencyEntry>, colormap: Option<Digest>, legend: Option<LegendLayoutRef>, labels: Option<EntityId> }`.
- **Algorithm:** canonical JSON per RFC 8785 using the canonicaliser that CORE.JRN uses for journal entries; sort collections by stable keys (entity ID, then plane ID); shortest round-trip f64 formatting (RFC 8785 number rules); SHA-256 digest.
- **Errors:** `Malformed`, `NonFiniteNumber`, `LimitExceeded`, `UnsupportedSchema`.
- **Determinism:** D0.
- **Performance budget:** serialise or parse a 10 000-entry ViewState under 10 ms [unverified].
- **Crate:** `fid-view` (module `state`).

## Test fixtures

`fixtures/viz/viewstate-samples`: hand-written JSON in-repo; generated cases from seed 3.

## Build prompt

> Create crate `fid-view` with module `state` defining `ViewState` (schema version, f64 camera eye/target/up, perspective or orthographic projection, clip planes, visibility rules, transparency entries, ColorMapSpec digest reference, legend layout reference, LabelSet reference). Implement RFC 8785 canonical JSON serialisation using the shared canonicaliser from the journal crate, deterministic ordering of collections by stable keys, shortest round-trip f64 output, and SHA-256 digest. Parsing is bounded (max 64 clip planes, 100 000 entries) and fails closed with typed errors (`Malformed`, `NonFiniteNumber`, `LimitExceeded`, `UnsupportedSchema`). Tests: byte round trip on fixture samples; seeded f64 bit-pattern property test; insertion-order independence; error cases; golden digest. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; no std HashMap in serialised paths.

## Open questions

- Whether the shared canonicaliser lives in CORE.JRN or a lower utility crate is CORE's call; this leaf only consumes it.

## Children

_Leaf: none._
