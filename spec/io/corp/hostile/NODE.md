---
id: IO.CORP.HOSTILE
title: Hostile corpus generator and expected errors
level: 3
parent: IO.CORP
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-io-corp]
built_pr: 154
satisfies: [REQ-IO.CORP-003]
covers: []
depends_on: [IO.CORP.MANIF, IO.REG.LIMITS]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.CORP.HOSTILE: Hostile corpus generator and expected errors

## Purpose

Generates, from each format's valid corpus files, a reproducible set of hostile variants (truncated, bit-flipped, oversized declarations, deep nesting, decompression bombs for compressed formats) and writes them into the format's manifest with the typed error each reader must return. Hostile coverage then grows automatically with every new valid file.

## Scope

**In:**
- Mutators: truncation at fixed fractions and at structure boundaries; header count inflation; seeded bit flips; nesting amplification for tree formats; ZIP and deflate bombs for container formats; billion-laughs XML for XML formats
- Per-format mutator selection declared by the format (tags in its descriptor)
- Expected-error assignment: a fixed table from mutator to error code, overridable per format with a reason
- Output: generated files in a cache plus manifest entries tagged `hostile` with `expect = reject{error}`

**Out:**
- Running readers (`IO.CORP.FUZZ`, `IO.CORP.SCORE`), manifest validation (`IO.CORP.MANIF`)

## Requirements

- **REQ-IO.CORP.HOSTILE-001** For every format, the generator shall produce at least truncated, malformed (bit-flipped) and oversized-declaration variants, and for compressed or container formats also decompression-bomb variants.
- **REQ-IO.CORP.HOSTILE-002** Each generated variant shall have an expected typed error in its manifest entry.
- **REQ-IO.CORP.HOSTILE-003** Given the same seed and inputs, the generator shall produce byte-identical variants.
- **REQ-IO.CORP.HOSTILE-004** Generated bombs shall be small on disk (≤ 1 MiB) and expand beyond the format's default byte limit.

## Acceptance criteria

- **AC-1** Given `fixtures/io/corp/hostile/cube_bin.stl` and seed 42, when the generator runs for format `mesh.stl`, then it emits ≥ 3 variants tagged `hostile` with expected errors `truncated_file`, `malformed` or `limit_exceeded`, and a second run gives identical digests.
- **AC-2** Given `fixtures/io/corp/hostile/cube_mm.3mf`, then a bomb variant ≤ 1 MiB is produced whose declared uncompressed size exceeds the default limit, expecting `limit_exceeded`.
- **AC-3** Given `fixtures/io/corp/hostile/min.qif` (XML), then a billion-laughs variant expecting `xml_forbidden_construct` is produced.
- **AC-4** Given a format with an override table entry (mutator `bitflip` → `checksum_mismatch` for E57), then that expected error is used and the reason is recorded.

## Design

- **Registry commands:** none (CI tool `corpus-hostile`).
- **Data structures:** `Mutator` enum; `MutatorPlan { format, mutators: [Mutator], overrides: [(Mutator, ErrorCode, reason)] }`.
- **Algorithm:** counter-based seeded RNG (CONSTITUTION §4 style) keyed by (seed, file digest, mutator, index); deterministic ZIP writer for bombs.
- **Errors:** `malformed` (bad plan).
- **Determinism:** D2 (seeded).
- **Performance budget:** < 1 min for the P0 corpus [unverified].
- **Crate:** `fid-io-corp`.

## Test fixtures

Small valid files under `fixtures/io/corp/hostile/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-corp`, add CI binary `corpus-hostile`: for each format manifest's accepted files, apply mutators (truncate, bit-flip, inflate declared counts, nesting amplification, ZIP/deflate bombs, XML billion-laughs) selected by the format's tags, using a counter-based RNG seeded by (seed, file digest, mutator, index); write variants to a cache and append manifest entries tagged `hostile` with `expect = reject{error}` from a mutator→error table with per-format overrides and reasons. Bombs ≤ 1 MiB on disk. Output must be byte-identical for the same seed. No panics, no unwrap (CONSTITUTION §8). Tests AC-1..AC-4.

## Open questions

- STL count expectations (for the IO.MESH.STL spec owner): the generator labels a flipped or maximal facet count `truncated_file`, following REQ-IO.MESH.STL-001/002 (size consistency decides binary versus ASCII, then a short file is `truncated_file`). Should a declared count whose 84 + 50·n exceeds the byte limit be `limit_exceeded` instead? [unverified] until an STL reader exists; see `fixtures/io/corp/hostile/plan/hostile.toml`.

## Children

_Leaf: none._
