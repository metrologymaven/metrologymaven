---
id: GDT.PMI.DIFF
title: PMI re-import diff
level: 3
parent: GDT.PMI
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.PMI-006]
covers: [GOM.GDT.16]
depends_on: [GDT.PMI.MAP, GDT.PMI.LINK]
side_effect: W1
determinism: D0
cli: [fid gdt propose-changes-from-pmi]
mcp: [gdt_propose_changes_from_pmi]
journal: [gdt.propose_changes_from_pmi@1]
updated: 2026-11-16
---

# GDT.PMI.DIFF: PMI re-import diff

## Purpose

When a new revision's PMI is imported, compares it with the adopted characteristics of the plan and proposes changes (added, changed, removed) on a draft or agent branch, without touching anything already adopted. Matching uses the `PmiLink` entity ID; entities whose IDs changed between revisions show as a removal plus an addition, which is safe and visible.

## Scope

**In:** matching by `pmi_entity_id`; classification `added | changed{fields} | removed | unchanged`; field-level comparison of the mapped characteristic (FCF, nominal, governing spec, links); a change proposal set adopted through `GDT.PMI.REVIEW`; `gdt.propose_changes_from_pmi@1` (W1).
**Out:** mapping PMI to characteristics (owned by `GDT.PMI.MAP`); adoption (owned by `GDT.PMI.REVIEW`); geometric matching when entity IDs change (not in scope; see Open questions).

## Requirements

- **REQ-GDT.PMI.DIFF-001** When PMI is re-imported for a revision, the system shall classify every adopted characteristic with a `PmiLink` and every new PMI entity as added, changed, removed or unchanged by entity ID.
- **REQ-GDT.PMI.DIFF-002** For a changed item, the diff shall list each differing field with old and new canonical values.
- **REQ-GDT.PMI.DIFF-003** The command shall write the diff as a change proposal set on a draft or agent branch and shall not modify any adopted characteristic.
- **REQ-GDT.PMI.DIFF-004** The diff output shall be ordered by entity ID so identical inputs give byte-identical output.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/pmi/rev_a_adopted.json` (four adopted characteristics) and `rev_b_pmi.json` (one tolerance value changed, one entity removed, one added), when `fid gdt propose-changes-from-pmi --plan <p> --pmi <handle> --json` runs, then the diff lists 1 changed (field `tolerance.value` old and new), 1 removed, 1 added, 2 unchanged.
- **AC-2** After AC-1, the four adopted characteristics are byte-identical to before.
- **AC-3** Given the same inputs twice, then the outputs are byte-identical.
- **AC-4** Given an entity whose ID changed but whose content did not, then it appears as one removal and one addition.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `gdt.propose_changes_from_pmi@1`, W1 (draft or agent branch). Params: `plan: PlanId`, `pmi: ImportHandle`. Result: `ChangeProposalSet`. `--json`, `--dry-run`.
- **Data structures:** `ChangeItem { entity_id, kind, fields: [FieldChange{path, old, new}] }`.
- **Algorithm:** map new PMI through `GDT.PMI.MAP` into proposals; join with adopted characteristics on entity ID (`BTreeMap`); compare canonical JSON per field path.
- **Errors:** `plan_unknown`, `not_an_import_handle`, pass-through mapping errors.
- **Determinism:** D0.
- **Performance budget:** 5 000 entities in < 500 ms [unverified].
- **Crate:** `fid-gdt`.

## Test fixtures

Synthetic adopted plans and PMI entity lists (CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, implement `gdt.propose_changes_from_pmi@1` (W1; CLI `fid gdt propose-changes-from-pmi`; MCP `gdt_propose_changes_from_pmi`; `--json`, `--dry-run`; agents pass import handles, never raw paths). Map the re-imported PMI with the existing PMI-to-proposal mapper, join with the plan's adopted characteristics on `PmiLink.pmi_entity_id`, and classify each as added, changed (list field paths with old and new canonical values), removed or unchanged, ordered by entity ID. Write the result as a change proposal set on a draft or agent branch; never modify adopted characteristics. Tests per the acceptance criteria, including byte-identical repeat output and exact replay. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`.

## Open questions

- CAD systems may regenerate PMI entity IDs between revisions [unverified]; geometric or semantic re-matching would be a later feature with its own node.

## Children

_Leaf: none._
