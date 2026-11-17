---
id: ECO.RULES.BLOCK
title: Generated block format and digest library
level: 3
parent: ECO.RULES
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.RULES-002]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.RULES.BLOCK: Generated block format and digest library

## Purpose

Defines the exact text format of a generated house-rules block inside a Markdown file (begin and end markers carrying the block ID, source revision and SHA-256 of the body) and provides the small library that renders, finds and parses blocks. SYNC and CHECK both use it, so the format is defined once.

## Scope

**In:**
- Marker syntax and the digest rule
- `render(block_id, source_rev, body) -> str`, `find_blocks(text) -> list`, `parse` with typed errors

**Out:**
- Writing files (owned by `ECO.RULES.SYNC`)
- Pass/fail policy (owned by `ECO.RULES.CHECK`)

## Requirements

- **REQ-ECO.RULES.BLOCK-001** A block shall start with the line `<!-- house-rules:begin block=<id> source=<repo-id>@<40-hex rev> sha256=<64-hex> -->` and end with the line `<!-- house-rules:end block=<id> -->`, and the body shall be every line between them.
- **REQ-ECO.RULES.BLOCK-002** The digest shall be the SHA-256 of the body encoded as UTF-8 with LF line endings, with no trailing newline added or removed.
- **REQ-ECO.RULES.BLOCK-003** If begin and end markers are unbalanced, nested, or their block IDs differ, then parsing shall fail with a typed error naming the line number.
- **REQ-ECO.RULES.BLOCK-004** Rendering then parsing a block shall return the same block ID, revision, digest and body, for any body that contains no marker line.

## Acceptance criteria

- **AC-1** Given `fixtures/blocks/one.md` (text, one valid block, text), when `find_blocks` runs, then it returns one block with the recorded ID, revision and digest, and `sha256(body)` equals the recorded digest.
- **AC-2** Given a body with CRLF line endings, then the computed digest equals that of the same body with LF endings.
- **AC-3** (error path) Given `fixtures/blocks/unbalanced.md` (begin without end), then parsing raises `BlockError("unbalanced", line=<n>)`.
- **AC-4** (error path) Given `fixtures/blocks/nested.md`, then parsing raises `BlockError("nested", line=<n>)`.
- **AC-5** (round trip) For 200 generated bodies from a seeded generator (seed 42), render then parse returns identical fields.

## Design

- **Registry command(s):** none; library module.
- **Data structures:** `Block(id, source_repo, source_rev, sha256, body, begin_line, end_line)`; `BlockError(kind, line)`.
- **Algorithm:** line scan with two compiled regular expressions; normalise CRLF to LF before hashing; block IDs match `^[a-z][a-z0-9-]{1,31}$`.
- **Errors:** `unbalanced`, `nested`, `id-mismatch`, `bad-marker`.
- **Determinism:** D0.
- **Performance budget:** under 10 ms for a 100 KB file.
- **Crate:** not Rust; Python package `tools/house_rules/` (module `block.py`), standard library only, so it runs in every repository's CI.

## Test fixtures

Hand-written Markdown files under `tools/tests/fixtures/blocks/` and a seeded body generator.

## Build prompt

> Create the Python package `tools/house_rules/` (stdlib only) with `block.py`: `render(block_id, source_repo, source_rev, body)`, `find_blocks(text) -> list[Block]`, `digest(body)`, and `BlockError(kind, line)`. Marker lines: `<!-- house-rules:begin block=<id> source=<repo-id>@<40-hex> sha256=<64-hex> -->` and `<!-- house-rules:end block=<id> -->`; body is the lines between; digest is SHA-256 of the body with CRLF normalised to LF. Errors: unbalanced, nested, id-mismatch, bad-marker, each with the 1-based line number. Write `unittest` tests for AC-1..AC-5 of spec node ECO.RULES.BLOCK.

## Open questions

- None.

## Children

_Leaf: none._
