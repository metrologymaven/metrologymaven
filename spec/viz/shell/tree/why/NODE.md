---
id: VIZ.SHELL.TREE.WHY
title: Why? provenance panel host
level: 4
parent: VIZ.SHELL.TREE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.TREE-004, REQ-VIZ.SHELL.TREE-005]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, VIZ.SHELL.A11Y.AXTREE, VIZ.SHELL.TREE.PROBLEMS, UNC.UEC.EXPL, CORE.JRN.ENTRY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.TREE.WHY: Why? provenance panel host

## Purpose

The answer to "why is this value what it is?" for any value of record or NOT_DECIDABLE result: algorithm ID and version, kernel version and manifest digest, input digests, evaluation basis, uncertainty statement and UEC level, decision rule with structured standards citation, the journal entry, and a replay action. Explanation content comes from the owning domains as data; this panel lays it out, keeps standards references structured (designator, edition, clause; never text) and exposes them to the accessibility tree.

## Scope

**In:** panel `viz.why`; opening from any `value_of_record` or `not_decidable` node; sections for provenance, uncertainty and decision; `Explanation` data intake (`UNC.UEC.EXPL` and other providers through an `ExplanationSource` trait); replay action (submits the replay command); fix actions from `FixService`; `explanation` role with `standards_ref` and `message_id` attributes.
**Out:** explanation content (`UNC.UEC.EXPL`, `UNC.DEC.DEFAULT`, `MIG.EXPLAIN`); the replay engine (`CORE.RPL`); fix catalogue (`FLOW.DEADEND.CATALOG`).

## Requirements

- **REQ-VIZ.SHELL.TREE.WHY-001** For a selected ResultRecord the panel shall show algorithm ID and version, kernel version, input digests, evaluation basis, UEC level, decision rule ID and outcome, spec context and journal entry hash, all from the record.
- **REQ-VIZ.SHELL.TREE.WHY-002** Every standards-based explanation shall show and expose a structured citation (designator, edition, clause) as the `standards_ref` attribute, and the panel shall never display standards text.
- **REQ-VIZ.SHELL.TREE.WHY-003** Every explanation sentence shall be rendered from a catalog message ID carried as the `message_id` attribute; content strings shall be rendered as untrusted text.
- **REQ-VIZ.SHELL.TREE.WHY-004** The panel shall offer a replay action carrying the replay command in its `command` attribute, and the fix actions of any catalogued reason code.
- **REQ-VIZ.SHELL.TREE.WHY-005** If a record has UEC-0, then the panel shall show "not evaluated" and shall never show a fabricated uncertainty.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/results-500` and a CONFORMS ASME characteristic, when Why? opens, then every field of REQ-001 is shown and equals the record.
- **AC-2** Given an explanation with a citation, then the node has `standards_ref = { designator, edition, clause }` and no text beyond catalog messages (supports `FLOW.DEADEND.WHY` `WHY-UNCITED` and `WHY-UNCATALOGUED-TEXT`).
- **AC-3** Given a UEC-0 record, then the uncertainty section reads "not evaluated".
- **AC-4** Given the replay action, then its `command` attribute names the replay command and activation opens its form.

## Design

- **Registry command(s):** none of its own.
- **Data structures:** `Explanation { sections: [{ title: MessageId, lines: [{ message: MessageId, args, standards_ref: Option<StdRef> }] }] }`; `trait ExplanationSource { fn explain(&self, record: &ResultRecordRef) -> Option<Explanation>; }`.
- **Errors:** missing explanation shows record fields only with "no explanation available".
- **Determinism:** D0.
- **Performance budget:** open under 50 ms.
- **Crate:** `fid-shell` (module `tree::why`).

## Test fixtures

`fixtures/viz/results-500` with explanation fixtures.

## Build prompt

> In crate `fid-shell`, add `tree::why`: a Why? panel (`viz.why`) opened from any value-of-record or NOT_DECIDABLE node, showing the record's algorithm ID/version, kernel version, input digests, evaluation basis, UEC level ("not evaluated" for UEC-0, never fabricated), decision rule and outcome, spec context and journal entry hash; explanation sections from `ExplanationSource` providers rendered from catalog message IDs (`message_id` attribute) with structured citations (`standards_ref` designator/edition/clause, never standards text); a replay action and catalogued fix actions with `command` attributes. Tests AC-1..AC-4. Rules: CONSTITUTION §5 (no standards text), §8; C6.

## Open questions

- None.

## Children

_Leaf: none._
