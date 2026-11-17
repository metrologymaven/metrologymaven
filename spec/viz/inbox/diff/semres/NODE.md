---
id: VIZ.INBOX.DIFF.SEMRES
title: Semantic and result diff layers from kernel records
level: 4
parent: VIZ.INBOX.DIFF
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.INBOX.DIFF-002]
covers: []
depends_on: [CORE.JRN.BR.DIFF, AGENT.PROPOSE.IMPACT, GDT.CHAR.RECORD, KRN.NUM.FMT.REG, VIZ.OVERLAY.STATUS]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.INBOX.DIFF.SEMRES: Semantic and result diff layers from kernel records

## Purpose

Builds the semantic layer (entities added, changed, removed, and conflicts) from the branch diff `journal.diff_branch@1`, and the result layer (characteristics whose value, decision or uncertainty class changed) from the computed impact `agent.compute_impact@1` plus the old and new ResultRecords it names. Every number shown is the stored f64 of a ResultRecord formatted by the kernel formatter; nothing is recomputed here, and no number from the agent's brief is used.

## Scope

**In:** `SemanticLayer` and `ResultLayer` types; mapping `EntityDelta` to rows with entity kind and name (name as untrusted text); loading ResultRecords by digest through `gdt.get_result@1`; formatting values and limits with `fid_num::fmt` using the formatter ID stored with each characteristic; decision-change highlighting (status glyphs from `VIZ.OVERLAY.STATUS`); the 4x4 decision-move matrix.
**Out:** computing the diff (`CORE.JRN.BR.DIFF`) and impact (`AGENT.PROPOSE.IMPACT`); the command assembling all layers (`VIZ.INBOX.DIFF.SHOW`); visuals (`VIZ.INBOX.DIFF.VISUAL`).

## Requirements

- **REQ-VIZ.INBOX.DIFF.SEMRES-001** The semantic layer shall list exactly the added, removed and changed entities of `journal.diff_branch@1` for the proposal's source and target, in the diff's order, with each conflict flagged.
- **REQ-VIZ.INBOX.DIFF.SEMRES-002** The result layer shall list exactly the characteristics in the `agent.compute_impact@1` result, each with old and new outcome, old and new UEC class, and old and new values read from the stored ResultRecords.
- **REQ-VIZ.INBOX.DIFF.SEMRES-003** Every displayed value shall be the string returned by the kernel formatter for the ResultRecord's stored f64 value and unit, together with the formatter ID.
- **REQ-VIZ.INBOX.DIFF.SEMRES-004** A row whose decision outcome changes shall be marked `decision_changed`, and rows shall be ordered with decision changes first, then by stable entity ID.
- **REQ-VIZ.INBOX.DIFF.SEMRES-005** If a ResultRecord named by the impact cannot be loaded, then the row shall show `record_missing` and no value.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/inbox-resdiff` (agent branch re-aligning a part so 3 of 20 characteristics change, one from CONFORMS to NONCONFORMS), when the layers are built, then the semantic layer equals the fixture's expected `journal.diff_branch@1` output and the result layer holds 3 rows with the decision change first.
- **AC-2** Given the same fixture, then each displayed value string equals `fid_num::fmt::format` of the stored f64 with the recorded formatter ID (property test over all rows).
- **AC-3** (error path) Given one record digest removed from the store, then that row shows `record_missing` with no value.
- **AC-4** Given the same inputs twice, then the layers serialise to byte-identical canonical JSON.

## Design

- **Registry command(s):** none; library used by `view.show_proposal@1`.
- **Data structures:** `SemanticRow { id, kind, name: Untrusted<String>, change: Added|Removed|Changed, conflict }`; `ResultRow { id, name: Untrusted<String>, old: Option<ShownValue>, new: Option<ShownValue>, old_outcome, new_outcome, old_uec, new_uec, decision_changed, status_glyphs }`; `ShownValue { text, unit, formatter_id, provenance: Computed }`; `moves: [[u32; 4]; 4]` copied from the impact.
- **Algorithm:** call diff and impact in-process; load records; format; sort.
- **Errors:** `unknown_branch`, `no_target` (passed through).
- **Determinism:** D0.
- **Performance budget:** 5,000 changed characteristics in < 300 ms including the impact [unverified until measured].
- **Crate:** `fid-inbox` (module `diff::semres`).

## Test fixtures

`fixtures/viz/inbox-resdiff`: generated journal and store from a scripted session; CC-BY-4.0.

## Build prompt

> In crate `fid-inbox`, add module `diff::semres`. Build the semantic layer from `journal.diff_branch@1` (added, removed, changed, conflicts, diff order) and the result layer from `agent.compute_impact@1` plus the old and new ResultRecords loaded by digest (`gdt.get_result@1`). Show every value only as the kernel formatter's string (`fid_num::fmt::format`) for the stored f64 and unit, with the formatter ID and provenance `Computed`; mark decision changes and sort them first, then by entity ID; show `record_missing` when a record is absent. Names are untrusted text. Tests AC-1..AC-4. Rules: no value of record computed or re-rounded here (C1); untrusted text is data (C6); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Resolves the parent's question: the result diff is computed by `AGENT.PROPOSE.IMPACT` over `CORE.JRN.BR.DIFF`, with values read from stored ResultRecords; no new kernel service is needed.

## Children

_Leaf: none._
