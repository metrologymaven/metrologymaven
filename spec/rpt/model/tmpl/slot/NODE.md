---
id: RPT.MODEL.TMPL.SLOT
title: Slot, binding-rule and repetition-rule language
level: 4
parent: RPT.MODEL.TMPL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.TMPL-002, REQ-RPT.MODEL.TMPL-003, REQ-RPT.MODEL.TMPL-004, REQ-RPT.MODEL.TMPL-009]
covers: []
depends_on: [RPT.MODEL.TMPL.FORMAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.MODEL.TMPL.SLOT: Slot, binding-rule and repetition-rule language

## Purpose

Defines what a slot may say: a binding rule that selects items (characteristics, elements, tags, stages, runs, a saved catalog query, or a table of a form data source), a sort key and a repetition rule (one row, one column group or one page group per item). The rules are typed data, never strings to evaluate, so a template cannot carry code. Evaluation against a project is done by `RPT.MODEL.BIND.EXPAND`; this leaf owns the types, validation and the ordering contract.

## Scope

**In:** `Slot`, `BindingRule`, `SortKey`, `RepetitionRule` types and their JSON forms; validation (closed enums, bounded lists, no free expressions); the ordering contract (sort key, then ascending stable ID); column repetition with `max_columns_per_group` and `repeat_columns`.

**Out:** evaluating rules at a journal head (`RPT.MODEL.BIND.EXPAND`); catalog query definitions (`RPT.CATALOG.QUERY`); the enclosing document format (`RPT.MODEL.TMPL.FORMAT`).

## Requirements

- **REQ-RPT.MODEL.TMPL.SLOT-001** A binding rule shall be one of `characteristics` (all, by IDs, or by type), `elements`, `tags` (any-of or all-of), `stages`, `runs`, `saved_query` (by `SavedQueryId`) or `field_table` (a table ID of a form data source), optionally intersected with an outcome filter, and shall contain no free-text expression.
- **REQ-RPT.MODEL.TMPL.SLOT-002** A slot shall name a sort key from a closed list (characteristic ID, characteristic name, balloon number, element ID, stage order, run start time) with ascending or descending direction, defaulting to ascending characteristic ID.
- **REQ-RPT.MODEL.TMPL.SLOT-003** A repetition rule shall be `row`, `column` (with `max_columns_per_group` from 1 to 64 and an optional list of `repeat_columns`) or `page_group` (naming the pages to repeat).
- **REQ-RPT.MODEL.TMPL.SLOT-004** The ordering contract shall state that items with equal sort keys are ordered by ascending stable record ID, so every order is total.
- **REQ-RPT.MODEL.TMPL.SLOT-005** If a slot is invalid (unknown enum value, empty ID list, more than 10 000 listed IDs, a `page_group` naming a missing page, or a `row` rule on a block that is not a table), then validation shall fail with error `report.slot_invalid` naming the block.
- **REQ-RPT.MODEL.TMPL.SLOT-006** All identifiers and tag names in a slot shall be parsed as untrusted content and only compared, never interpreted.

## Acceptance criteria

- **AC-1** Given `fixtures/rpt/model/tmpl/slots_valid.json` (one slot of each rule and repetition kind), when validated, then all pass and round-trip byte-identically.
- **AC-2** (error path) Given `fixtures/rpt/model/tmpl/slots_invalid.json` (one case per REQ-005 condition), when validated, then each fails with `report.slot_invalid` naming its block.
- **AC-3** (ordering) Given a property test with 10 000 random item lists (seed 9) and equal sort keys, when sorted by the contract function, then the order equals ascending stable ID and is independent of input order.
- **AC-4** (C6) Given a tag name containing prompt-like text and bidi controls, when validated, then it is stored as untrusted and matching compares bytes only.

## Design

- **Registry commands:** none; library types plus `validate(slot, block, page_ids) -> Result<(), SlotError>` and `order(items, key) -> Vec<Item>` used by EXPAND.
- **Data structures:** `Slot { rule: BindingRule, sort: SortKey, repeat: RepetitionRule }`; enums as listed in the requirements; ID lists as `Vec<StableId>` (sorted, unique).
- **Errors:** `report.slot_invalid { block, reason }`.
- **Determinism:** D0; total order defined here.
- **Performance budget:** order 100 000 items in under 50 ms [unverified until measured].
- **Crate:** `fid-report` (module `tmpl::slot`).

## Test fixtures

`fixtures/rpt/model/tmpl/slots_valid.json`, `slots_invalid.json` (hand-written, CC-BY-4.0); seeded property test.

## Build prompt

> In crate `fid-report`, add module `tmpl::slot`: types `Slot { rule, sort, repeat }`, `BindingRule` (`characteristics | elements | tags | stages | runs | saved_query | field_table`, optional outcome filter; no free-text expressions), `SortKey` (closed list, direction; default ascending characteristic ID), `RepetitionRule` (`row | column { max_columns_per_group 1-64, repeat_columns } | page_group { pages }`); `validate` (`report.slot_invalid` naming the block for unknown values, empty or >10 000 ID lists, missing pages, row rule on a non-table block) and `order` (sort key, then ascending stable ID: a total order). All identifiers untrusted, compared only. Acceptance: AC-1..AC-4 in `spec/rpt/model/tmpl/slot/NODE.md`. Constraints: CONSTITUTION C6, §8; no std HashMap/HashSet.

## Open questions

- `field_table` and column repetition were added for the fixed form templates (`RPT.FORMS.LAYOUT.*`).

## Children

_Leaf: none._
