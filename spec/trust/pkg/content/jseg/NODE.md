---
id: TRUST.PKG.CONTENT.JSEG
title: Journal segment and checkpoint choice
level: 4
parent: TRUST.PKG.CONTENT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG.CONTENT-004, REQ-TRUST.PKG.CONTENT-005, REQ-TRUST.PKG.CONTENT-008]
covers: []
depends_on: [TRUST.PKG.MANIFEST, CORE.JRN.CHAIN.APPEND, CORE.JRN.CHAIN.CKPT]
uses_artifacts: [CORE.JRN.ENTRY, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PKG.CONTENT.JSEG: Journal segment and checkpoint choice

## Purpose

Chooses the slice of the Record Journal that goes into a package: from the nearest checkpoint at or before the first entry that produced an included result (or from the session header when there is none) through the last such entry, with no gaps, plus the checkpoint files that anchor it. Detached (redacted) payload fields stay out, but their salted commitments stay in, so `TRUST.CHAIN` can still verify the chain offline.

## Scope

**In:**
- Finding first and last referenced entries from the records' journal entry hashes
- Choosing the anchoring checkpoint or the session header
- Emitting `journal/segment.jsonl` and `journal/checkpoints/<index>.json`
- Excluding redacted detachable bodies while keeping commitments

**Out:**
- Record selection (`TRUST.PKG.CONTENT.SELECT`)
- Creating checkpoints and appending entries (`CORE.JRN.CHAIN.CKPT`, `CORE.JRN.CHAIN.APPEND`)
- Checking the chain (`TRUST.CHAIN`)

## Requirements

- **REQ-TRUST.PKG.CONTENT.JSEG-001** The segment shall start at the latest checkpoint whose sequence is at or before the earliest entry referenced by any included record and shall end at the latest referenced entry, including every entry of that stream and branch in between.
- **REQ-TRUST.PKG.CONTENT.JSEG-002** If no checkpoint exists at or before the earliest referenced entry, then the segment shall start at the `session_header` entry of the session that contains that entry.
- **REQ-TRUST.PKG.CONTENT.JSEG-003** If a record's journal entry hash is not found in the journal, then segment choice shall fail with `pkg.entry_missing` naming the record and hash.
- **REQ-TRUST.PKG.CONTENT.JSEG-004** For every entry whose detachable payload field is redacted in the source journal, the segment shall omit the field body and keep its salted commitment unchanged, so that the recomputed entry hash equals the stored one.
- **REQ-TRUST.PKG.CONTENT.JSEG-005** The segment shall be written as `journal/segment.jsonl`, one RFC 8785 canonical entry per line, and each anchoring checkpoint as `journal/checkpoints/<index>.json` in CORE's checkpoint canonical form.
- **REQ-TRUST.PKG.CONTENT.JSEG-006** If the referenced entries span more than one stream or branch, then segment choice shall fail with `pkg.segment_split` naming them [unverified: whether multi-branch packages are ever needed].

## Acceptance criteria

- **AC-1** Given golden journal `fixtures/journal/golden/small.jsonl` (checkpoints at 10 and 40) and records referencing entries 23 and 37, when `choose` runs, then the segment is entries 10..37 and checkpoint 10 is included.
- **AC-2** Given records referencing entries 3..5 before the first checkpoint, then the segment starts at the session header (entry 0).
- **AC-3** Given fixture `fixtures/pkg/content/redacted` (entry 25 with a redacted payload field), then the exported entry has no body for that field, keeps the commitment, and `TRUST.CHAIN.CANON` recomputes the stored hash.
- **AC-4** (error path) Given a record whose entry hash is unknown, then `pkg.entry_missing`; given records on two branches, then `pkg.segment_split`.

## Design

- **Registry command(s):** none.
- **Data structures:** `JournalSegment { stream, branch, first_seq, last_seq, entries: Vec<CanonicalEntryBytes>, checkpoints: Vec<CheckpointBytes> }`.
- **Algorithm:** map entry hashes to sequences via the journal index; take min/max; pick the anchoring checkpoint or the session header; stream entries in sequence order; for redacted fields keep header and commitment only (CORE.JRN.ENTRY's header/payload separation).
- **Errors:** `pkg.entry_missing`, `pkg.segment_split`.
- **Determinism:** D0.
- **Performance budget:** 1,000,000-entry segment streamed in < 5 s [unverified until measured].
- **Crate:** `fid-pkg`, module `content::jseg`, feature `export`.

## Test fixtures

CORE.JRN golden journals (`fixtures/journal/golden/*`), plus `fixtures/pkg/content/redacted` generated from them with one redacted field.

## Build prompt

> In crate `fid-pkg`, feature `export`, add module `content::jseg` with `pub fn choose(journal: &dyn JournalRead, records: &[ResultRecord]) -> Result<JournalSegment, PkgError>`. Resolve each record's journal entry hash to (stream, branch, sequence) (`pkg.entry_missing` if absent; `pkg.segment_split` if more than one stream or branch). Start at the latest checkpoint at or before the earliest referenced sequence, else at the session's `session_header` entry; end at the latest referenced sequence; include every entry between. Write entries as RFC 8785 canonical JSON lines (`journal/segment.jsonl`) and anchoring checkpoints as `journal/checkpoints/<index>.json` in CORE's checkpoint canonical form. For redacted detachable fields keep the salted commitment and omit the body, so entry hashes still verify. Tests: AC-1..AC-4 using CORE.JRN golden journals. Constraints: CONSTITUTION §6 (RFC 8785 + SHA-256), §8, D0, streaming (no full-journal load).

## Open questions

- If CORE later allows packages spanning branches (after promotion), REQ-006 can be relaxed by a minor format version.

## Children

_Leaf: none._
