---
id: ALN.MODEL.ACTIVE
title: Active alignment and result stamping
level: 3
parent: ALN.MODEL
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ALN.MODEL-005]
covers: []
depends_on: [ALN.MODEL.ELEM, ALN.MODEL.HIER, CORE.PRJ]
side_effect: W2
determinism: D0
cli: [fid align set-active]
mcp: [align_set_active]
journal: [align.set_active@1]
updated: 2026-11-16
---

# ALN.MODEL.ACTIVE: Active alignment and result stamping

## Purpose

Holds which alignment is active in a document and provides the stamp that every result records: the ID and content digest of the alignment it was evaluated in. Evaluating domains (GDT, SCAN, CMM) call the stamp function; this leaf guarantees that no result needing an alignment is evaluated without one.

## Scope

**In:**
- The document-level active-alignment pointer and `align.set_active@1`
- The `AlignmentStamp` value and the `stamp_for_evaluation` function that evaluators call
- Refusal when no alignment is active and the evaluation needs one

**Out** (and which node owns it instead):
- The alignment element (`ALN.MODEL.ELEM`); composition to root (`ALN.MODEL.HIER`)
- Release blocking for unadopted or non-converged alignments (`ALN.MODEL.ADOPT`, status from solvers)
- The ResultRecord type itself (`CORE`, `GDT.CHAR`); this leaf supplies the stamp fields

## Requirements

- **REQ-ALN.MODEL.ACTIVE-001** When `align.set_active` runs with an alignment ID, the system shall make that alignment the document's active alignment and journal one event; with `none`, it shall clear the pointer.
- **REQ-ALN.MODEL.ACTIVE-002** If the named alignment has status `failed` or does not exist, then `align.set_active` shall reject with `alignment_failed` or `not_found` and journal nothing.
- **REQ-ALN.MODEL.ACTIVE-003** When an evaluator requests a stamp, the system shall return the active alignment's ID, content digest, status and composed-to-root transform digest, and the evaluator shall store them on the result.
- **REQ-ALN.MODEL.ACTIVE-004** If no alignment is active and the evaluation declares `needs_alignment`, then the stamp request shall fail with `no_active_alignment` and the evaluation shall produce no result.
- **REQ-ALN.MODEL.ACTIVE-005** When the active alignment changes, the system shall not modify, mark or recompute results already stamped with another alignment; their dependency is on the stamped alignment, not on the pointer.
- **REQ-ALN.MODEL.ACTIVE-006** When the stamped alignment has status `not_converged`, `ambiguous` or `outdated`, the stamp shall carry that status so that the result shows it.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/aln/model/chain3`, when `fid align set-active <A2> --json` runs, then the active pointer is A2 and the Record Journal gains exactly one `align.set_active@1` event.
- **AC-2** Given `chain3` with A2 active, when a test evaluator requests a stamp, then it receives A2's ID, digest, status `ok` and the digest of A0·A1·A2 composed.
- **AC-3** (error path) Given `chain3` with no active alignment, when a test evaluator declaring `needs_alignment` runs, then it fails with `no_active_alignment` and no result element exists.
- **AC-4** (error path) Given an alignment with status `failed`, when `fid align set-active` names it, then the command fails with `alignment_failed` and journals nothing.
- **AC-5** Given a result stamped with A1, when the active alignment changes to A2, then the result's stamp, status and digest are unchanged.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `align.set_active@1` (W2), param `alignment: AlignmentId | none`; result `{ previous, active }`.
- **Data structures:** `ActiveAlignment(Option<AlignmentId>)` stored in the document; `AlignmentStamp { alignment_id, alignment_digest, status, root_transform_digest }`.
- **Algorithm:** `stamp_for_evaluation(doc, needs_alignment)` reads the pointer, fetches the element, calls `ALN.MODEL.HIER` `compose_to_root`, digests the canonical JSON of the composed transform.
- **Errors:** `not_found`, `alignment_failed`, `no_active_alignment`.
- **Determinism:** D0; no floating-point work except the composition already covered by GEO.FRAME (D0).
- **Performance budget:** stamp in under 10 µs for chains up to 64 levels [unverified until measured].
- **Crate:** `fid-align` (provisional name).

## Test fixtures

`fixtures/aln/model/chain3` (see `ALN.MODEL.ELEM`), plus a test evaluator that declares `needs_alignment`. Project-generated, CC-BY-4.0.

## Build prompt

> In crate `fid-align`, implement the document's active-alignment pointer, the registry command `align.set_active@1` (class W2; CLI `fid align set-active <id|none> [--json]`; MCP `align_set_active`; journal `align.set_active@1`) and the function `stamp_for_evaluation(doc, needs_alignment) -> Result<AlignmentStamp, AlignError>` that evaluators call.
> Requirements: set_active points to an existing, not-failed alignment or clears it; one journal event per call; errors `not_found` and `alignment_failed` journal nothing. The stamp holds alignment ID, content digest, status and the SHA-256 digest of the canonical JSON of its composed-to-root transform (use `compose_to_root` from the hierarchy module). With no active alignment and `needs_alignment`, return `no_active_alignment`. Changing the active alignment never touches existing results. Non-ok statuses (not_converged, ambiguous, outdated) are carried in the stamp.
> Acceptance: fixture `fixtures/aln/model/chain3`; tests for set-active journaling, stamp contents for A2, refusal with no active alignment, refusal of a failed alignment, existing results unchanged after a pointer change, exact replay IDENTICAL.
> Constraints: no panic, no unwrap/expect, no unsafe; no std HashMap/HashSet; agents calling this over MCP write to their agent branch (W2).

## Open questions

- Per-group or per-data-set active alignments (as in some incumbents) are not included; a result can always be evaluated in an explicitly named alignment by the evaluator passing an ID instead of using the pointer.

## Children

_Leaf: none._
