---
id: RPT.MODEL.BIND.EXPAND
title: Slot, repetition and multi-stage expansion
level: 4
parent: RPT.MODEL.BIND
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BIND-006]
covers: [GOM.RPT.07]
depends_on: [RPT.MODEL.TMPL.SLOT, RPT.MODEL.BIND.RESOLVE, CORE.PRJ.STAGE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.MODEL.BIND.EXPAND: Slot, repetition and multi-stage expansion

## Purpose

Turns the slots of a template or report into concrete bindings at a journal head: it evaluates each slot's binding rule into an ordered item list, emits one table row, one column group or one page group per item as the repetition rule says, and lays out results from several stages or runs of one project side by side in a fixed order. Its output is a list of expanded blocks with plain bindings that the resolver can look up.

## Scope

**In:** evaluation of `BindingRule`s (characteristic, element, tag, stage, run, saved catalog query reference, `FieldSource` table) into item lists; total ordering by the rule's sort key with the stable-ID tiebreak; row, column-group and page-group repetition; multi-stage and multi-run columns in stage order then run time order; deterministic IDs for expanded blocks.

**Out:** the slot and rule language and its parser (`RPT.MODEL.TMPL.SLOT`); ID lookup and errors (`RPT.MODEL.BIND.RESOLVE`); resolved document and digest (`RPT.MODEL.BIND.GEN`); catalog query execution (`RPT.CATALOG.QUERY`, P1b, called through a trait).

## Requirements

- **REQ-RPT.MODEL.BIND.EXPAND-001** When a slot is expanded at a journal head, the system shall evaluate its binding rule to an item list ordered by the rule's sort key and then by ascending stable ID, so the order is total.
- **REQ-RPT.MODEL.BIND.EXPAND-002** A row-repetition slot shall emit one table row per item, a column-repetition slot one column (or column group) per item, and a page-group slot one copy of its page group per item, in item order.
- **REQ-RPT.MODEL.BIND.EXPAND-003** When a report binds results from several stages or runs of one project, the expanded columns shall appear in stage order as defined by the project, then in ascending run start time, then by run ID.
- **REQ-RPT.MODEL.BIND.EXPAND-004** Each expanded block shall get a derived UUIDv5 ID computed from the template block ID and the item's stable ID, so regenerating gives the same IDs (CONSTITUTION §5).
- **REQ-RPT.MODEL.BIND.EXPAND-005** If a slot's rule references a saved catalog query and no catalog query provider is installed, then expansion shall fail with error `report.query_unavailable` naming the slot.
- **REQ-RPT.MODEL.BIND.EXPAND-006** If expansion would produce more than 100 000 rows or 5 000 pages for one report [unverified limits], then it shall fail with error `report.expansion_too_large` naming the slot.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage` and the starter multi-stage template, when expanded, then the table has 50 rows in ascending characteristic ID and three stage columns in project stage order.
- **AC-2** Given two characteristics with equal sort keys, when expanded, then they appear in ascending stable-ID order, and shuffling insertion order in the fixture does not change the output.
- **AC-3** Given a page-group slot over 4 elements, when expanded twice, then 4 page groups are produced with identical derived UUIDv5 block IDs both times.
- **AC-4** (error path) Given a slot referencing a saved query with no provider, when expanded, then it fails with `report.query_unavailable`.
- **AC-5** (error path) Given a rule selecting 100 001 items into rows, then it fails with `report.expansion_too_large`.

## Design

- **Registry commands:** none; library API `expand(template_or_report, ctx: &ResolveContext, query: Option<&dyn QueryProvider>) -> Result<Vec<ExpandedBlock>, ExpandError>`.
- **Data structures:** `ExpandedBlock { id: BlockId (UUIDv5), source_block: BlockId, item: Option<Ref>, binding: Binding }`; `trait QueryProvider { fn run(&self, q: SavedQueryId, head: JournalHash) -> Result<Vec<Ref>, ...>; }`.
- **Algorithm:** for each block in document order: no slot -> copy; slot -> evaluate the rule against the project model at `head` (stages from `CORE.PRJ.STAGE`), sort by (sort key, stable ID), check limits, emit expanded blocks with UUIDv5(namespace = template block ID, name = item ID).
- **Errors:** `report.query_unavailable`, `report.expansion_too_large`, rule evaluation errors from `RPT.MODEL.TMPL.SLOT`.
- **Determinism:** D0; ordered collections, no clock, derived IDs.
- **Performance budget:** expand 500 characteristics by 3 stages in under 50 ms [unverified until measured].
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (shared), a shuffled variant generated in the test with seed 11.

## Build prompt

> In crate `fid-report`, implement slot expansion: `expand(doc, ctx, query_provider?) -> Vec<ExpandedBlock>`.
>
> **Rules:** evaluate each slot's binding rule at `ctx.head` into items ordered by (sort key, stable ID); row slots emit rows, column slots columns or column groups, page-group slots copies of the page group; multi-stage and multi-run columns in project stage order, then run start time, then run ID; expanded block IDs are UUIDv5(template block ID, item ID); saved-query rules need a `QueryProvider` (else `report.query_unavailable`); more than 100 000 rows or 5 000 pages -> `report.expansion_too_large`.
>
> **Acceptance:** AC-1..AC-5 in `spec/rpt/model/bind/expand/NODE.md`.
>
> **Constraints:** CONSTITUTION §5 (derived IDs are UUIDv5), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no std HashMap/HashSet; no clock reads.

## Open questions

- The expansion limits are placeholders [unverified]; confirm with the PDF render budget.

## Children

_Leaf: none._
