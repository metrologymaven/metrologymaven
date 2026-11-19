---
id: GEO.FRAME.TREE
title: Frame tree and chain resolution
level: 3
parent: GEO.FRAME
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-geo-frame]
built_pr: 240
satisfies: [REQ-GEO.FRAME-001, REQ-GEO.FRAME-002]
covers: []
depends_on: [GEO.FRAME.XFORM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# GEO.FRAME.TREE: Frame tree and chain resolution

## Purpose

The per-document tree of coordinate frames: one world root, every other frame with exactly one parent and one transform to it, cycle-free edits, and resolution of the transform between any two frames by composing through their lowest common ancestor in a fixed order.

## Scope

**In:**
- `Frame { id, name, kind, parent, to_parent }` with kinds world, part, instrument, alignment and user
- `FrameTree` value with insert, reparent, set transform and remove operations, each returning a new tree or a typed error
- Cycle rejection and single-world-root invariant
- `resolve(a, b) -> Transform` mapping coordinates in frame a to frame b
- A tree digest for replay checks

**Out:**
- Registry commands and journaling (owned by `GEO.FRAME.CMD`)
- Transform arithmetic (owned by `GEO.FRAME.XFORM`)
- Which elements reference a frame (owned by CORE element tree; checked in `GEO.FRAME.CMD`)

## Requirements

- **REQ-GEO.FRAME.TREE-001** The frame tree shall have exactly one frame of kind world, with no parent, created with the tree.
- **REQ-GEO.FRAME.TREE-002** If an insert or reparent would make a frame its own ancestor, then the operation shall fail with `Cycle { path }` and return the tree unchanged.
- **REQ-GEO.FRAME.TREE-003** When `resolve(a, b)` is called, the tree shall find the lowest common ancestor c, compose the transforms from a up to c in child-to-parent order, compose the inverses from c down to b, and return the result.
- **REQ-GEO.FRAME.TREE-004** If a frame to be removed still has child frames, then removal shall fail with `HasChildren`.
- **REQ-GEO.FRAME.TREE-005** The tree shall store frames in an ordered map keyed by frame ID, so iteration order and the tree digest are platform-independent.
- **REQ-GEO.FRAME.TREE-006** `resolve(a, a)` shall return the exact identity, and `resolve(a, b)` composed with `resolve(b, a)` shall map any point within 100 m back within 1e-12 m.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/geo/frame/tree_7.json` (world, two parts, two instruments, two alignments, depth 4), when `resolve` runs for all 49 ordered pairs, then each result matches the fixture's expected matrix as IEEE hex on x86-64, ARM64 and wasm32.
- **AC-2** (error path) Given `tree_7`, when frame `part_a` is reparented under its own descendant `align_1`, then the result is `Cycle` with the path `[part_a, align_1]` and the tree digest is unchanged.
- **AC-3** (error path) Given `tree_7`, when `part_a` (which has children) is removed, then the result is `HasChildren`.
- **AC-4** Given any pair from `tree_7` and 10⁵ seeded points, when mapped a→b→a, then the maximum error is ≤ 1e-12 m.
- **AC-5** Given the same edits applied in the same order on two platforms, when the tree digest is computed, then it is identical.

## Design

- **Registry command(s):** none; library leaf (commands are in GEO.FRAME.CMD).
- **Data structures:** `FrameId([u8; 16])`; `FrameKind`; `Frame`; `FrameTree { frames: BTreeMap<FrameId, Frame>, world: FrameId }`.
- **Algorithm:** ancestor lists by walking parents (depth bounded by frame count); LCA as the last common entry of the two root-first lists; composition as `T_{c←a} = T_{c←p1} ∘ … ∘ T_{p_k←a}`, evaluated left-fold from a upward; then `T_{b←a} = inverse(T_{c←b}) ∘ T_{c←a}`.
- **Errors:** `UnknownFrame`, `Cycle { path }`, `HasChildren`, `WorldImmutable`.
- **Determinism:** D0.
- **Performance budget:** resolve on a 1 000-frame tree of depth 20 under 20 µs [unverified].
- **Crate:** `fid-geo-frame` (kernel graph, KNP-1 applies).
- **Pinned choices (build, value-affecting).** The full text is in the module docs of `crates/fid-geo-frame/src/tree.rs` and is part of this design:
  - Chains are left folds from the frame upward (`acc = T(g).compose(acc)`). An empty chain is absent, never an identity multiplied in (that could flip the sign of a zero). `a = b` gives the exact identity; only a's chain: `T_{c←a}`; only b's chain: `inverse(T_{c←b})`; both: `inverse(T_{c←b}).compose(T_{c←a})`.
  - The world frame created by `new()` has ID `FrameId::WORLD` (16 zero bytes) and name `world`. `FrameId` text is 32 hex digits; upper- and lower-case parse to the same bytes, so case-only variants are the same ID (`DuplicateId`).
  - Names: 1 to 255 UTF-8 bytes, no control characters, unique after ASCII lower-casing (`BadName`, `DuplicateName`).
  - Every `to_parent` is re-checked on entry with XFORM's rotation check (composition does not re-check, so a composed chain can drift): not orthonormal or non-finite → `BadTransform`; determinant not +1 → `Handedness` (a reflection would join frames of opposite handedness).
  - Bounds: depth (edges from world) ≤ 256 (`TooDeep`), at most 2^20 frames (`TooManyFrames`); every walk is an explicit loop bounded by these, scratch vectors use `try_reserve` (`Alloc`).
  - Edits take `&mut self`, validate fully, then mutate; on any error the tree and its digest are unchanged. `reparent(id, new_parent, to_parent)` takes the new transform, since the old one is relative to the old parent. `Cycle { frames }` (the field is `frames`, not `path`: the kernel implementation-digest scan (KRN.MAN.MANI.IMPL) refuses the identifier `path` in kernel-closure sources) lists the frame, its would-be parent and that frame's ancestors up to the frame (excluded): AC-2 gives `[part_a, align_1]`.
  - Extra errors beyond the four above: `DuplicateId`, `DuplicateName`, `BadId`, `BadName`, `NoParent`, `SecondWorld`, `WorldCount`, `BadWorld` (for `from_frames`, which rebuilds a tree from frames in any order and checks dangling parents and cycles), `Handedness`, `BadTransform`, `TooDeep`, `TooManyFrames`, `Alloc`, `Xform`, `Corrupt`.
  - Digest layout version 1: SHA-256 (fid-kci) over `"FIDFRT" | 0x01 | n u64 | world id | per frame in ID order: id | kind u8 | has_parent u8 | parent [16] | name_len u16 | name | xform u8 (1 rigid, 2 similarity) | R 9 f64 | t 3 f64 | s f64`, little-endian, `-0.0` written as `+0.0`; kind bytes world 1, part 2, instrument 3, alignment 4, user 5.
  - Path round-trip bound (extends XFORM REQ-004 to a path of k rigid edges with residuals ρ_i and translations t_i): `|x' − x| ≤ (Σρ_i)·M + 48·2⁻⁵³·(2k + 4)·M`, `M = |x| + Σ|t_i|`; `FrameTree::path_round_trip_bound` evaluates it, `None` for a path with a similarity edge. Derivation in the module docs; the independent script checks it in exact arithmetic on XFORM's worst-residual rotations. (Build note on REQ-006 and AC-4: as with XFORM REQ-004, the 1e-12 m figure cannot hold for every accepted rotation, since the ρ term alone reaches about 1e-10 m at 100 m; it holds for `tree_7`, whose transforms are orthonormal to rounding.)
  - AC-4 is run as 10⁵ seeded points in the 100 m cube, point k on ordered pair k mod 49.
  - REQ-GEO.FRAME-008 (frame-mismatch guard) is not in this leaf: GEO.FRAME's Children table assigns it to APPLY and CMD.

## Test fixtures

- `fixtures/geo/frame/tree_7.json`: hand-designed tree with transforms from `xform_cases.json`; expected matrices computed by the independent Python script. CC-BY-4.0.

## Build prompt

> In crate `fid-geo-frame` (pure Rust, no I/O, KNP-1), implement module `tree`: `FrameId([u8;16])`, `FrameKind { World, Part, Instrument, Alignment, User }`, `Frame { id, name, kind, parent: Option<FrameId>, to_parent: Transform }`, and `FrameTree { frames: BTreeMap<FrameId, Frame>, world }` with `new()` (creates the single world frame), `insert`, `reparent`, `set_transform`, `remove`, `resolve(a, b) -> Result<Transform, FrameError>` and `digest()`. Reject cycles (`Cycle{path}`), removing frames with children (`HasChildren`), editing the world frame (`WorldImmutable`) and unknown IDs. `resolve` composes from a up to the lowest common ancestor in child-to-parent order, then applies the inverse chain down to b; `resolve(a,a)` is exact identity. Acceptance: AC-1 all 49 pairs of `tree_7.json` bit-exact on three targets; AC-2 cycle rejected with path and unchanged digest; AC-3 `HasChildren`; AC-4 round trip ≤ 1e-12 m; AC-5 identical digest across platforms. Constraints: CONSTITUTION §8 (no panic, no `unsafe`), KNP-1 (BTreeMap, no HashMap, no FMA). Uses `Transform` from module `xform`. Files: `crates/fid-geo-frame/src/tree.rs`, `tests/tree.rs`, `fixtures/geo/frame/tree_7.json`.

## Open questions

- None.

## Children

_Leaf: none._
