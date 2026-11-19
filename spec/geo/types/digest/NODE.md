---
id: GEO.TYPES.DIGEST
title: Canonical byte layout and element digest
level: 3
parent: GEO.TYPES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-geo-types]
built_pr: 198
satisfies: [REQ-GEO.TYPES-002, REQ-GEO.TYPES-005]
covers: []
depends_on: [GEO.TYPES.ENV, GEO.TYPES.POINTS, GEO.TYPES.MESH]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.TYPES.DIGEST: Canonical byte layout and element digest

## Purpose

A content digest for every geometric element, computed over a documented canonical byte layout, so that indexes, selections, dependency tracking and replay can tell whether two elements are the same geometry on any platform.

## Scope

**In:**
- The canonical layout (version 1) for `PointSet`, `PointCloud`, `Mesh` and `CadBodyRef`
- SHA-256 over that layout, streamed so no full copy of the element is made
- `ElementDigest` type with hex display
- Normalisation rules (signed zero in f64 and f32 fields; NaN canonicalised to one quiet NaN per width, so the digest never fails)

**Out:**
- Journal entry hashing (owned by `CORE.JRN`)
- Blob addressing in the store (owned by `CORE.STO`)
- Chunked cloud digests (owned by `GEO.OOC`, which must reproduce this digest on round trip)

## Requirements

- **REQ-GEO.TYPES.DIGEST-001** The digest shall be SHA-256 over a layout that starts with the ASCII tag `FIDGEO` and a layout version byte, followed by the element kind byte, the 16-byte frame ID, 64-bit little-endian counts, coordinates as little-endian IEEE 754 f64 bits, triangle indexes as u64 little-endian, and attribute channels in ascending channel-name byte order.
- **REQ-GEO.TYPES.DIGEST-002** Before hashing, the layout shall replace -0.0 by +0.0 in every f64 field, so numerically equal elements have equal digests.
- **REQ-GEO.TYPES.DIGEST-003** The digest of a mesh shall not depend on whether its triangle indexes are stored as u32 or u64.
- **REQ-GEO.TYPES.DIGEST-004** Two elements that differ in any coordinate bit (after signed-zero normalisation), index, attribute value or frame ID shall have different digests, up to SHA-256 collision.
- **REQ-GEO.TYPES.DIGEST-005** The digest computation shall stream the element and shall allocate no more than 1 MiB of extra memory regardless of element size.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/types/digest_vectors.json` (pointset_small, mesh_cube, cloud_grid_2scans with their expected SHA-256 hex), when digests are computed on x86-64, ARM64 and wasm32 in CI, then every digest matches the vector exactly.
- **AC-2** Given `pointset_small` and a copy with one coordinate changed by one ULP, when both are digested, then the digests differ.
- **AC-3** Given `pointset_small` with point 0's x set to -0.0 and a copy with +0.0, when digested, then the digests are equal.
- **AC-4** Given `mesh_cube` in u32 and u64 index storage, when digested, then the digests are equal.
- **AC-5** Given `pointset_small` in two different frames, when digested, then the digests differ.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `pub struct ElementDigest(pub [u8; 32])`; `pub fn digest(e: &GeometryElement) -> ElementDigest`.
- **Algorithm:** a streaming writer feeds SHA-256 through one fixed 64 KiB block buffer (no copy of the element). The encoding below is permanent for layout version 1; any change needs a new version byte.
- **Layout v1** (every integer little-endian; every variable-length part is prefixed by its own count):
  - Header: `"FIDGEO" | 0x01 | kind u8 | frame[16]`. Kind bytes: point set 1, point cloud 2, mesh 3, CAD body handle 4.
  - Point set, point cloud, mesh: `n_points u64 | x[n_points] f64 | y[n_points] f64 | z[n_points] f64 | n_tris u64 | n_tris × (a u64, b u64, c u64) | n_channels u64 | channels`. Point kinds write `n_tris = 0`. Triangle indexes are always written as u64, whatever the storage width.
  - Each channel, in ascending name-byte order: `name_len u16 | name (ASCII) | type u8 | n_records u64 | records`. A channel present with zero records is written and differs from an absent one.
  - CAD body handle: header, then `body[16] | source_digest[32 raw bytes]`.
- **Channels:**

  | name | type byte | records | written when |
  |---|---|---|---|
  | `colour` | 3 `u8x3` (r, g, b) | one per point | present |
  | `grid` | 7 `u64` | two: rows, cols | a grid is declared |
  | `intensity` | 2 `f32` | one per point | present |
  | `normal` | 8 `f64x3col` (all x, then all y, then all z) | one per point | present |
  | `quality` | 4 `u16` | one per point | present |
  | `scan_origins` | 9 `origin` (scanner ID u32, then x, y, z f64) | one per origin | at least one origin |
  | `scanner_id` | 5 `u32` | one per point | present |
  | `timestamp_ns` | 6 `i64` (two's complement) | one per point | present |
  | `triangle_quality` | 4 `u16` | one per triangle (meshes) | present |
  | `triangle_scanner_id` | 5 `u32` | one per triangle (meshes) | present |

  Type byte 1 (`f64`) is reserved for a scalar f64 channel; none uses it yet.
- **Floats:** every f64 is written as its IEEE 754 bits, except -0.0 → +0.0 and any NaN → `0x7ff8000000000000`. Every f32 likewise: -0.0 → +0.0, any NaN → `0x7fc00000`; infinities keep their bits. The rules act on bit patterns, never on float arithmetic. Valid elements hold no NaN or infinity in f64 fields (constructors reject them), so the f64 NaN rule exists only so the function is total; f32 intensity is not validated today, so its NaN rule is live (maintainer decision: keep canonicalisation).
- **Not hashed:** topology facts (derived from the triangles) and the import-time AABB of a CAD body handle.
- **Interfaces:** `ElementDigest` displays as 64 lowercase hex and converts to the shared `fid_types::Digest` (`sha256:` form); `write_layout` streams the layout for consumers that must reproduce it (GEO.OOC).
- **Errors:** none; every valid element can be digested.
- **Determinism:** D0. Integer and byte operations only.
- **Performance budget:** at least 500 MB/s of layout bytes on reference hardware [unverified].
- **Crate:** `fid-geo-types`, using the portable FIPS 180-4 SHA-256 of the first-party kernel crate `fid-kci` (no third-party dependency; the kernel allowlist stays empty).

## Test fixtures

- `fixtures/geo/types/digest_vectors.json`: expected digests for the GEO.TYPES fixtures, generated once by an independent Python script (`hashlib`) that writes the same layout, and reviewed. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-types` (pure Rust, no I/O, kernel graph), add module `digest` with `ElementDigest([u8;32])` and `digest(&GeometryElement) -> ElementDigest`, computing SHA-256 (`fid_kci::kci::Sha256`, first-party kernel crate) over this streamed layout v1: ASCII `FIDGEO`, version byte 0x01, kind byte, 16-byte frame ID, u64 LE point count, x column, y column, z column as LE f64 bits with -0.0 normalised to +0.0, u64 LE triangle count and indexes as u64 LE (regardless of storage width), u64 LE channel count, then channels in ascending name-byte order as `name_len u16 | name | type byte | n_records u64 | records` (channel table and float rules as in Design: -0.0 → +0.0 and NaN canonicalised for f64 and f32). `CadBodyRef` hashes body ID and source digest. Stream in 64 KiB blocks, extra memory ≤ 1 MiB. Acceptance: test vectors in `digest_vectors.json` match on x86-64, ARM64 and wasm32; one-ULP change alters the digest; ±0.0 give equal digests; u32/u64 storage equal; different frame gives a different digest. Also write the independent Python generator `tools/fixtures/geo_digest_vectors.py` (hashlib) used to produce the vectors. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (no std HashMap/HashSet). Files: `crates/fid-geo-types/src/digest.rs`, `tests/digest.rs`, `fixtures/geo/types/digest_vectors.json`, `tools/fixtures/geo_digest_vectors.py`.

## Open questions

- Whether CORE.STO's blob digest and this element digest should coincide for elements stored as one blob; currently they are independent.
- Whether GEO.TYPES.POINTS should reject non-finite intensity values at construction, as it does for coordinates and normals. Until it does, the f32 NaN canonicalisation above is what keeps such elements digestible and platform-stable.

## Children

_Leaf: none._
