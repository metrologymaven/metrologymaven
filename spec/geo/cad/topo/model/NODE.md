---
id: GEO.CAD.TOPO.MODEL
title: B-rep arena, typed IDs, builder and canonical encoding
level: 4
parent: GEO.CAD.TOPO
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
built_in: [fid-geo-cad]
satisfies: [REQ-GEO.CAD.TOPO-001, REQ-GEO.CAD.TOPO-006]
covers: []
depends_on: [GEO.NURBS.DATA, GEO.NURBS.ANALYTIC, GEO.TYPES.ENV]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GEO.CAD.TOPO.MODEL: B-rep arena, typed IDs, builder and canonical encoding

## Purpose

The in-memory B-rep model every CAD operation reads: an arena of bodies, shells, faces, loops, coedges, edges and vertices over GEO.NURBS curve and surface definitions, with typed stable IDs, a builder that importers (IO.CAD) call, and a versioned canonical binary encoding with a SHA-256 digest so a body can be cached, compared and reloaded bit-identically. It is pure L0 library code (no CORE, no I/O).

## Scope

**In:**
- Arena types: `Body`, `Shell`, `Face { surface, sense, loops }`, `Loop { coedges, outer }`, `Coedge { edge, sense, pcurve: Option<Curve2>, loop }`, `Edge { curve: Option<Curve3>, start, end, param_range }`, `Vertex { point }`
- Surfaces and curves held as GEO.NURBS `SurfaceDef` / `CurveDef` (NURBS or analytic)
- `TopologyId`: the imported label when the source gives one, else a UUIDv5
- Canonical index path and UUIDv5 derivation
- `BodyBuilder` with structural checks (references in range, loops non-empty), no geometric validation
- Canonical encoding v1, bounded decoder, `body_digest`
- Dense per-kind indexes (`FaceIx`, `EdgeIx`, ...) in canonical order for use as `IndexSet` domains

**Out** (and which node owns it instead):
- Adjacency queries (owned by `GEO.CAD.TOPO.ADJ`)
- Missing pcurve derivation (owned by `GEO.CAD.TOPO.PCURVE`)
- Trim classification (owned by `GEO.CAD.TOPO.TRIM`)
- Geometric validation (owned by `GEO.CAD.TOPO.VALID`)
- Commands (owned by `GEO.CAD.TOPO.CMD`)
- Parsing STEP and IGES into builder calls (owned by `IO.CAD.STEP`)

## Requirements

- **REQ-GEO.CAD.TOPO.MODEL-001** When the builder is given an imported label for a face, loop, coedge, edge or vertex, the model shall use that label (UTF-8, 1 to 256 bytes, held as untrusted data per C6) as the entity's `TopologyId`.
- **REQ-GEO.CAD.TOPO.MODEL-002** When no imported label is given, the model shall derive the ID as UUIDv5 over a fixed project namespace and the bytes `body_id | kind_code | canonical index path`, where the index path is (shell, face, loop, coedge) positions and edges and vertices take the path of their first use in that traversal order.
- **REQ-GEO.CAD.TOPO.MODEL-003** If two entities of one body would get the same `TopologyId`, then `build` shall fail with `DuplicateId` naming the ID and the two index paths.
- **REQ-GEO.CAD.TOPO.MODEL-004** If a builder call references an entity index that does not exist or a loop has no coedges, then `build` shall fail with `BadReference` naming the referring entity.
- **REQ-GEO.CAD.TOPO.MODEL-005** The model shall store all entities and lookup tables in vectors and `BTreeMap`s only (no hash containers), so iteration order is a function of the builder input alone.
- **REQ-GEO.CAD.TOPO.MODEL-006** Building the same builder call sequence twice shall produce byte-identical canonical encodings and the same `body_digest` on every platform.
- **REQ-GEO.CAD.TOPO.MODEL-007** If an encoded body exceeds 10⁷ entities or 1 GiB, or its bytes are malformed, then `decode` shall fail closed with `TooLarge` or `Malformed` and never panic [unverified limits].
- **REQ-GEO.CAD.TOPO.MODEL-008** `TopologyId` shall have a total order (imported labels before UUIDs, then bytewise), used by every ascending-ID output in GEO.CAD.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/cad/topo/cube_labelled.json` (a unit cube description with STEP-style labels on every entity), when built, then every entity ID equals its label and `body_digest` equals the committed value.
- **AC-2** Given fixture `fixtures/geo/cad/topo/cube_unlabelled.json`, when built twice and on Linux, Windows and macOS CI, then the derived IDs and the encoding are byte-identical everywhere and match the committed vector.
- **AC-3** (error path) Given a description with two faces labelled `#12`, when built, then `DuplicateId` names `#12` and both paths.
- **AC-4** (error path) Given a coedge referencing edge index 99 in a 12-edge body, when built, then `BadReference` names that coedge.
- **AC-5** (error path) Given 200 truncated or bit-flipped copies of the AC-1 encoding (seed 7), when decoded, then each returns `Malformed` or `TooLarge` and the fuzz harness records no panic.
- **AC-6** Given the AC-1 body, when encoded, decoded and re-encoded, then the two encodings are identical.

## Design

- **Registry command(s):** none; library leaf.
- **Data structures:** `TopologyId { Imported(BoundedStr<256>), Derived([u8; 16]) }`; `Body { id: [u8; 16], shells: Vec<Shell>, faces: Vec<Face>, loops: Vec<Loop>, coedges: Vec<Coedge>, edges: Vec<Edge>, vertices: Vec<Vertex>, ids: BTreeMap<TopologyId, EntityIx> }`; `Sense { Same, Reversed }`; `BodyBuilder` with `add_vertex`, `add_edge`, `add_face`, `add_loop`, `add_coedge`, `add_shell`, `build() -> Result<Body, TopoBuildError>`.
- **Algorithm:** after structural checks, canonicalise entity order by the traversal (shell, face, loop, coedge; edges and vertices by first use), assign IDs, then build the ID map. Encoding v1: magic `FIDBREP`, version byte, counts as u64 LE, entities in canonical order, f64 as raw IEEE bits, curve and surface definitions in the GEO.NURBS.DATA field order. Digest: SHA-256 of the encoding (same `sha2` crate as GEO.TYPES.DIGEST).
- **Errors:** `DuplicateId`, `BadReference`, `EmptyLoop`, `TooLarge`, `Malformed`, `UnsupportedVersion`.
- **Determinism:** D0 (integer and byte operations only).
- **Performance budget:** build plus encode of a 10⁵-face body under 300 ms [unverified].
- **Crate:** `fid-geo-cad` (L0, kernel graph, KNP-1 applies; UUIDv5 via an allowlisted `uuid` crate with the v5 feature [allowlist to confirm in ECO.RULES]).
- **Pinned at build (value-affecting choices; the independent Python reference `fixtures/geo/cad/topo/make_topo_fixtures.py` implements exactly these):**
  - *UUIDv5:* RFC 9562 §5.5 over a crate-private FIPS 180-4 SHA-1 (`topo::uuid`), not the `uuid` crate and not `fid-uuid`: `fid-uuid` is a model-layer crate built on RustCrypto `sha1`, which the kernel graph may reach neither by layer nor by allowlist. Namespace `0932dc8b-f062-5d17-b3eb-8089eddd9e6e` = UUIDv5 of `https://github.com/metrologymaven/metrologymaven/spec/geo/cad/topo/model#topology-id-v1` in the URL namespace. Name bytes: `body_id` (16) | kind code u8 (shell 1, face 2, loop 3, coedge 4, edge 5, vertex 6) | each path component as u32 little-endian.
  - *Index paths:* shell `[s]`, face `[s, f]`, loop `[s, f, l]`, coedge `[s, f, l, c]`, each a position in the parent's list; an edge takes the path of the first coedge using it; a vertex takes that coedge path plus `0` (it is the edge's start) or `1` (end) at its first use, so the two vertices of one edge never share a path. Canonical order: shells in builder order, faces, loops and coedges by that traversal; edges and vertices by first use (an edge's start before its end). Shells carry derived IDs only (REQ-001 lists no shell labels).
  - *Labels:* any UTF-8 of 1 to 256 bytes, stored verbatim; shown only escaped (`{:?}`), never raw (C6). Imported labels sort before derived IDs, then bytewise (derived `Ord`).
  - *Check order, first failure wins (typed `TopoBuildError`, indexes are builder indexes):* (0) total entities ≤ 10⁷ `TooLarge`; (1) per entity, kinds vertex, edge, coedge, loop, face, shell, each in builder order: label length `BadLabel`, finite point and range `NonFinite`, `t0 < t1` `BadParamRange`, references in range `BadReference`, `EmptyLoop`, at most one outer loop per face `MultipleOuter`, `EmptyShell`; (2) ownership, walking shells → faces → loops in builder order: a face, loop or coedge with a second owner is `Reused`, then any face, loop, coedge, edge or vertex not reached is `Orphan`; (3) loop closure: the end vertex of coedge k (by its sense) is the start vertex of coedge k + 1, cyclically, else `OpenLoop`; (4) pairing per edge: more than two coedges, two with the same sense, two in different shells, or one in a shell declared `closed` is `BadPairing`; (5) IDs inserted shells, faces, loops, coedges, edges, vertices in canonical order; the first repeat is `DuplicateId` with both index paths. These are combinatorial checks only; gaps, curve-on-surface distance and other geometric defects stay with GEO.CAD.TOPO.VALID. A face may have no loops (a closed periodic surface).
  - *Geometry:* faces hold `SurfaceDef` (plane, cylinder, cone, sphere, torus or NURBS), edges `Option<CurveDef>` (line, circle, ellipse or NURBS), coedges `Option<Curve2>` (2-D line `origin + t·dir` or a NURBS curve with every `z = 0`). Analytic definitions keep the parameters as given (frame `origin`, `axis`, `ref_dir`, then sizes in constructor order) next to the GEO.NURBS object built from them, because `Placement` re-normalises its axes and re-encoding the normalised axes would not round-trip bit-identically.
  - *Floats:* every `f64` is written as its IEEE 754 binary64 bits, little-endian. The model replaces `-0.0` by `+0.0` everywhere (points, ranges, frames, sizes, knots, control points, 2-D lines) when a definition or body is built, so equal geometry has one encoding. NaN and infinities are refused at construction (`NonFinite`, or GEO.NURBS errors in `GeomError`) and never written; on decode, NaN, infinity and `-0.0` bits are `Malformed` (`BadFloat`).
  - *Encoding v1* (integers little-endian, booleans and senses one byte `0`/`1`, sense `0` same): `"FIDBREP"` | version `0x01` | `body_id` (16) | counts u64 × 6 (shells, faces, loops, coedges, edges, vertices) | shells (`id` | `closed` | face count u64) | faces (`id` | sense | loop count u64 | surface) | loops (`id` | `outer` | coedge count u64) | coedges (`id` | edge index u64 | sense | pcurve) | edges (`id` | start u64 | end u64 | `t0` f64 | `t1` f64 | curve) | vertices (`id` | x | y | z) | trailer = SHA-256 of everything before it (32). Each shell owns the next face-count faces, each face the next loops, each loop the next coedges. `id` is `0x00` + 16 UUID bytes, or `0x01` + u16 length + UTF-8 label. Curve tags `0` absent, `1` line, `2` circle, `3` ellipse, `16` NURBS; surface tags `1` plane, `2` cylinder, `3` cone, `4` sphere, `5` torus, `16` NURBS; pcurve tags `0` absent, `1` 2-D line (origin 2 f64, dir 2 f64), `16` NURBS. Analytic payload: frame (9 f64) then sizes. NURBS curve in GEO.NURBS.DATA field order: degree u64 | knot count u64 | knots | control-point count u64 | points (3 f64, or 2 for a pcurve) | weights flag u8 (+ one f64 per control point) | `periodic`. NURBS surface: `degree_u` | `degree_v` | knots_u (count + values) | knots_v | `n_u` | `n_v` | control-point count | points | weights flag (+ weights) | `periodic_u` | `periodic_v`. A version-2 layout keeps the 32-byte SHA-256 trailer as its framing.
  - *Digest:* `body_digest` = SHA-256 (fid-kci's portable FIPS 180-4 code) of the whole encoding including the trailer, in the `sha256:` wire form (`fid-types::Digest`).
  - *Decoder order:* more than 1 GiB → `TooLarge`; shorter than header + trailer → `Malformed`; trailer ≠ SHA-256(payload) → `Malformed` (so every truncation or bit flip of AC-5 fails here, before any parsing); magic → `Malformed`; version ≠ 1 → `UnsupportedVersion`; counts summing above 10⁷ → `TooLarge`; counts whose minimum encoded size exceeds the remaining bytes, and NURBS vector lengths that exceed them, → `Malformed` before anything is allocated; then every field. The parsed description goes through `BodyBuilder::build` (a failure is `Malformed` carrying the `TopoBuildError`), and the result must re-encode to the input bytes, else `Malformed` (`NonCanonical`): non-canonical order, a wrong derived ID or a shell with a label is refused, so a decoded body is exactly as valid as a built one. Memory is bounded by a constant multiple of the input size (no separate allocation budget).

## Test fixtures

- `fixtures/geo/cad/topo/cube_labelled.json`, `cube_unlabelled.json`: hand-written JSON builder scripts (8 vertices, 12 edges, 6 planar faces), committed with expected digests. CC-BY-4.0.
- Fuzz corpus generated from AC-1 with seed 7.

## Build prompt

> In crate `fid-geo-cad` (pure Rust, L0, no I/O, no CORE dependency, KNP-1), add module `topo::model`. Implement arena types `Body`, `Shell`, `Face`, `Loop`, `Coedge`, `Edge`, `Vertex` over `fid-geo-nurbs` `CurveDef`/`SurfaceDef` (NURBS or analytic), `Sense`, and `TopologyId { Imported(label ≤ 256 UTF-8 bytes), Derived(uuid bytes) }` with a total order (imported first, then bytewise). Provide `BodyBuilder` whose `build()` checks references and non-empty loops (`BadReference`, `EmptyLoop`), puts entities in canonical traversal order (shell, face, loop, coedge; edges and vertices by first use), assigns IDs (imported label, else UUIDv5 over a fixed namespace and `body_id | kind_code | index path`) and fails with `DuplicateId` on collisions. Provide canonical encoding v1 (`FIDBREP` magic, LE u64 counts, raw f64 bits), a bounded fail-closed `decode` (≤ 10⁷ entities, ≤ 1 GiB) and `body_digest` (SHA-256). Use only `Vec` and `BTreeMap`. Acceptance: AC-1 labelled cube IDs and digest; AC-2 identical encodings on three OSes; AC-3 `DuplicateId`; AC-4 `BadReference`; AC-5 200 corrupted inputs fail without panic; AC-6 round trip. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny unwrap/expect), C6 for labels, §5 stable IDs. Files: `crates/fid-geo-cad/src/topo/model.rs`, `src/topo/encode.rs`, `tests/topo_model.rs`, `fixtures/geo/cad/topo/`.

## Open questions

- Whether IO.CAD.STEP writes the encoded body to CORE.STO as a cache next to the STEP bytes, or rebuilds on every load; either works with this encoding.

## Children

_Leaf: none._
