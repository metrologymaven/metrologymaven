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
built_in: [fid-view]
built_pr: 264
satisfies: [REQ-VIZ.VIEW-001, REQ-VIZ.VIEW-002]
covers: []
depends_on: [CORE.JRN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **AC-2** (property) Given 10 000 generated ViewStates (seed 3) with random f64 values including subnormals and ±0, when serialised and parsed, then every f64 has the identical bit pattern, except −0.0, which reads back as +0.0 (see Design).
- **AC-3** Given two ViewStates built with the same clip planes inserted in different orders, then their canonical bytes and digests are identical.
- **AC-4** (error path) Given a ViewState with a NaN camera coordinate, when parsed, then it fails with `NonFiniteNumber`; given 65 clip planes, it fails with `LimitExceeded`.
- **AC-5** Given a ViewState of schema version 1, then its digest matches the stored golden digest in the fixture.

## Design

- **Registry command(s):** none (data model).
- **Data structures:** `ViewState { schema: u16, camera: Camera { eye_m, target_m, up: [f64; 3], projection: Perspective { fov_y_rad } | Ortho { height_m }, near_m, far_m }, viewport: Viewport { width_px, height_px }, clips: Vec<ClipPlane { id: u32, normal, offset_m }>, visibility: Vec<VisibilityRule { entity, visible }>, transparency: Vec<TransparencyEntry { entity, alpha }>, colormap: Option<Digest>, legend: Option<LegendLayoutRef>, labels: Option<EntityId> }`. `EntityId` is the workspace UUID; `Digest` and `LegendLayoutRef` (the legend layout's canonical digest) are `sha256:` digests. `near_m`/`far_m` and `viewport` complete the camera and the "viewport layout" of the Purpose; multi-viewport layouts stay in MULTI.
- **Canonical form (schema 1):** one JSON object with exactly the keys `camera, clips, colormap, labels, legend, schema, transparency, viewport, visibility`; camera keys `eye_m, far_m, near_m, projection, target_m, up`; projection `{fov_y_rad, kind:"perspective"}` or `{height_m, kind:"ortho"}`; clip `{id, normal, offset_m}`; visibility `{entity, visible}`; transparency `{alpha, entity}`; viewport `{height_px, width_px}`. Every key is always present (absent references are `null`); unknown keys are `Malformed`. UUIDs are written lower case.
- **Float formatting (pinned):** every f64 is a plain JSON number in RFC 8785 §3.2.2.3 form (ECMAScript `Number::toString` of the shortest round-trip digits, exact ties to even), written by the CORE.JRN canonicaliser `fid_canon::format_number`, so parsing returns the identical bit pattern, subnormals included. Not the `0x` bit-string form of `F64Bits`: view state is meant to be readable and the shortest form already round-trips.
- **Negative zero (pinned):** `-0.0` is written `0` and parses as `+0.0`. The two zeros give the same view, so they give the same bytes and digest; `-0.0` is the only value whose bits do not survive a round trip. Lenient input `-0` is accepted and normalised.
- **Integers:** `schema`, clip `id` and viewport sizes are JSON numbers with no fraction (`1.0` and `1e0` are accepted by the lenient parser and written `1`).
- **Validation (fails closed, first fault, field path named):** non-finite numbers `NonFiniteNumber`; camera `DegenerateCamera` for zero up vector, eye equal to target (or a view direction that overflows), up parallel to the view direction (sine of the angle at most 1e-12, computed on vectors scaled by their largest component), `fov_y_rad` outside (0, π), perspective `near_m <= 0`, ortho `height_m <= 0`, `near_m >= far_m`; clip normals `DegenerateNormal` (zero) or `NonUnitNormal` (`|n·n − 1| > 1e-12`); `alpha` outside [0, 1] and viewport sizes outside 1..=32768 `OutOfRange`; repeated clip ID or entity `DuplicateKey`; references not held by the document `UnknownReference` (`check_references` against a caller-supplied `References`; `unknown_references` lists all of them for NAMED's restore report).
- **Algorithm:** canonical JSON per RFC 8785 using the canonicaliser that CORE.JRN uses for journal entries (`fid-canon`); sort collections by stable keys (clip planes by plane ID, visibility and transparency by entity ID); SHA-256 digest of the canonical bytes. `parse` accepts any JSON spelling and returns the state with collections sorted; `parse_canonical` additionally requires the input to be the canonical bytes (`NotCanonical`).
- **Limits:** at most 64 clip planes and 100 000 entries in each of `visibility` and `transparency` (`LimitExceeded`, checked before entries are decoded); input at most 24 MiB; JSON nesting at most 4; a lexical pre-scan bounds the number of separators (and so of values) before any tree is built and names the field of a `NaN`, `Infinity` or overflowing literal.
- **Errors:** `Malformed`, `NonFiniteNumber`, `LimitExceeded`, `UnsupportedSchema`, `DegenerateCamera`, `DegenerateNormal`, `NonUnitNormal`, `OutOfRange`, `DuplicateKey`, `UnknownReference`, `NotCanonical`, each with a stable code and a field path (`camera.eye_m[1]`, `clips[3].normal`, `$` for the whole document).
- **Determinism:** D0.
- **Performance budget:** serialise or parse a 10 000-entry ViewState under 10 ms [unverified].
- **Crate:** `fid-view` (module `state`).

## Test fixtures

`fixtures/viz/viewstate-samples`: 20 hand-written ViewStates with golden digests, 200 generated cases from seed 3, lenient (non-canonical) inputs with their canonical forms, and invalid inputs with hand-labelled error codes and fields. All are written by `make_fixtures.py`, an independent stdlib-Python RFC 8785 writer (`--check` runs in CI); the 10 000-state property test (AC-2) generates its own states from seed 3 in Rust.

## Build prompt

> Create crate `fid-view` with module `state` defining `ViewState` (schema version, f64 camera eye/target/up, perspective or orthographic projection, clip planes, visibility rules, transparency entries, ColorMapSpec digest reference, legend layout reference, LabelSet reference). Implement RFC 8785 canonical JSON serialisation using the shared canonicaliser from the journal crate, deterministic ordering of collections by stable keys, shortest round-trip f64 output, and SHA-256 digest. Parsing is bounded (max 64 clip planes, 100 000 entries) and fails closed with typed errors (`Malformed`, `NonFiniteNumber`, `LimitExceeded`, `UnsupportedSchema`). Tests: byte round trip on fixture samples; seeded f64 bit-pattern property test; insertion-order independence; error cases; golden digest. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; no std HashMap in serialised paths.

## Open questions

- Whether the shared canonicaliser lives in CORE.JRN or a lower utility crate is CORE's call; this leaf only consumes it.

## Children

_Leaf: none._
