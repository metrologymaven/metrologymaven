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
built_in: [fid-report]
built_pr: 274
satisfies: [REQ-RPT.MODEL.TMPL-002, REQ-RPT.MODEL.TMPL-003, REQ-RPT.MODEL.TMPL-004, REQ-RPT.MODEL.TMPL-009]
covers: []
depends_on: [RPT.MODEL.TMPL.FORMAT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
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
- **Pinned at build** (fid-report `tmpl::slot`; each rule has a test in `crates/fid-report/tests/tmpl_slot.rs`; fixtures and expected bytes, pointers, reasons and orders from `fixtures/rpt/model/tmpl/make_slot_fixtures.py`, checked in CI with `--check`):
  - *Envelope.* `RPT.MODEL.TMPL.FORMAT` stores a slot as `{"kind": <slot kind>, "spec": {"untrusted": true, "value": <object>}}`. This leaf defines the object, read at pointer `/spec/value`. It has exactly the keys `repeat`, `rule`, `sort`. As in the format, integers are canonical decimal strings, and the object holds no JSON number. Unknown members are refused everywhere, so a slot cannot carry an expression.
  - *Rule.* `rule` is `{<the envelope kind>: <selector>}` plus an optional `outcome` member. The selector key must equal the envelope kind; any other slot-kind word is `kind_mismatch`, and any other key is `unknown_field`. Selectors: `characteristics` is `"all"`, `{"ids": [...]}` or `{"type": <QIF characteristic type token>}`. `elements`, `stages` and `runs` are `"all"` or `{"ids": [...]}`. `tags` is `{"any_of": [...]}` or `{"all_of": [...]}`. `saved_query` is `{"id": <UUIDv7>}` (`SavedQueryId`). `field_table` is `{"table": <table ID>}`. A selector object holds exactly one member: an empty one is `missing_field` at the selector, and a second member is `conflicting_fields` at the later key. `outcome` is a non-empty list of `conforms`, `indeterminate`, `nonconforms`, `not_decidable` (CONSTITUTION §5) in strictly ascending code-point order. An absent `outcome` means no filter; `null` is not a spelling of it.
  - *Identifiers.* Every selector refers only by ID. A stable ID or tag name is untrusted content (C6): 1 to 256 bytes of UTF-8 with no NUL (`empty_id`, `id_too_long`, `id_nul`), otherwise kept exactly as written. It is never normalised, case-folded or interpreted, and it is compared byte for byte. Matching a tag selector compares bytes only. ID, tag and page lists hold 1 to 10 000 entries (`empty_list`, `too_many_ids`) in strictly ascending UTF-8 byte order (`duplicate`, `not_sorted`), so each slot has exactly one spelling. A table ID and a column key match `^[a-z][a-z0-9_]{0,63}$` (`bad_table_id`, `bad_column_key`). A characteristic type matches `^[A-Z][A-Za-z0-9]{0,63}$` (`bad_type_name`); this leaf checks only the shape, and membership in the QIF type list (`GDT.CHAR.MODEL.TYPES`) is checked at expansion. A saved query ID or page ID is canonical lowercase UUIDv7 text (`bad_id`).
  - *Sort.* `sort` is `null` (the default: ascending `characteristic_id`) or `{"direction": "asc" | "desc", "key": <field>}`. The fields are `characteristic_id`, `characteristic_name`, `balloon_number`, `element_id`, `stage_order` and `run_start_time`; any other value is `unknown_value`. A parsed slot keeps `null` as written, so it round-trips byte for byte, and `Slot::sort()` returns the effective key.
  - *Repeat.* `repeat` reads its `kind` tag first (`missing_field`, `unknown_value`), then its key set. `{"kind": "row"}` is allowed only on a block of kind `table` (`row_needs_table` at `/spec/value/repeat/kind`). The column form is `{"kind": "column", "max_columns_per_group": "1".."64", "repeat_columns": null | [...]}`. Its count is a canonical decimal (`bad_integer`, `out_of_range`). `repeat_columns` holds 1 to 64 distinct column keys (`empty_list`, `too_many_columns`, `duplicate`) and keeps the written display order. The page-group form is `{"kind": "page_group", "pages": [...]}`. Its page IDs are in ascending order, and each must be a page of the template (`page_not_found`). Copies follow the pages' document order at expansion.
  - *Order of checks.* Validation follows the format's rule. In each object it checks the key set first: the first unknown key in code-point order, then the first missing key. It then reads the members in code-point key order (`repeat`, `rule`, `sort`), depth first. List entries are read in index order, and each entry is checked for shape, then order, then (for pages) presence. `validate_template` visits blocks in document order and returns the first error.
  - *Error.* `report.slot_invalid { block, pointer, reason }`. `pointer` is RFC 6901, relative to the block's `slot` member, and is cut to 1024 bytes. In `--json` form the pointer is marked untrusted, and in the human form it is escaped like `Untrusted::escaped`. The reason words are the ones named above, plus `wrong_type`, `unknown_field` and `missing_field`.
  - *Ordering contract.* `compare(a, b, sort)` / `order(items, sort)` sort an `Item { id: StableId, value: SortValue }`, where `SortValue` is `Int(i64) | Text | Missing` and is supplied by EXPAND for the slot's field. The rules: (1) an item with a value comes before a `Missing` one, in both directions; (2) values compare in the slot's direction, `Int` by number and `Text` by UTF-8 bytes, with every `Int` before every `Text` in ascending direction (and after it in descending); (3) ties fall back to ascending stable ID by UTF-8 bytes, in both directions. That is the one collation: code-point order, with no locale, no normalisation and no case folding (UTF-16 order is not used). Items that compare equal are identical, so the result depends only on the input multiset. The algorithm is a stable `sort_by`, O(n log n).
  - *Checked constructors.* No public type can hold a value the loader refuses. `RepetitionRule::Column` and `RepetitionRule::PageGroup` wrap `ColumnRule` and `PageGroupRule`, whose fields are private. `ColumnRule::new`, `PageGroupRule::new`, `SortedList::new` (ID and tag lists), `BindingRule::new` and `Slot::new` all return `Result<_, BuildError>`. They refuse exactly what `validate` refuses, with the same reason word. Lists are not sorted for the caller: unsorted input is `not_sorted`. `Slot::new` also refuses a spec value over the format's 65 536-byte payload bound (`value_too_large`), so `Slot::to_format` cannot fail. Page presence and `row` on a table block need the template, so they stay in `validate`.
  - *API.* `validate(&format::Slot, &TemplateBlock, &BTreeSet<PageId>) -> Result<Slot, SlotError>` returns the typed slot rather than `()`. The other entry points are `validate_template(&Template) -> Result<Vec<(BlockId, Slot)>, SlotError>`, `Slot::{to_value, to_envelope_value, to_format}` and `order` / `compare`. `BindingRule` is a struct `{ selector: Selector, outcome }` whose `Selector` enum holds the seven kinds.
  - *Fixtures.* These live in `fixtures/rpt/model/tmpl/`. `slots_valid.json` is one template with 13 slots, one of each rule and repetition kind, including hostile IDs and tags. Its canonical bytes are in `slots_valid.canonical.json`, and the per-slot canonical bytes and effective sort are in `slots_valid.expected.json`. `slots_invalid.json` holds 47 cases with hand-labelled block, code, pointer and reason; one of them, a JSON number inside a spec value, is refused by the format layer (`report.template_invalid`, reason `number`) before any slot rule runs. `order_cases.json` holds 45 ordering cases with the expected order from an independent Python comparator. Every valid template fixture in that folder, including `RPT.MODEL.TMPL.FORMAT`'s `valid_v1.json` and `v0_sample.json` (whose generator now writes a byte-sorted tag list and a v1-shaped slot spec), must also pass slot validation. AC-3 uses SplitMix64 with seed 9: 10 000 lists, each with one shared sort value, checked against a plain byte sort of the IDs and against a Fisher-Yates shuffle.

## Test fixtures

`fixtures/rpt/model/tmpl/slots_valid.json`, `slots_invalid.json`, `order_cases.json` (synthetic, CC-BY-4.0, written by `make_slot_fixtures.py`; reasons and pointers hand-labelled); seeded property test.

## Build prompt

> In crate `fid-report`, add module `tmpl::slot`: types `Slot { rule, sort, repeat }`, `BindingRule` (`characteristics | elements | tags | stages | runs | saved_query | field_table`, optional outcome filter; no free-text expressions), `SortKey` (closed list, direction; default ascending characteristic ID), `RepetitionRule` (`row | column { max_columns_per_group 1-64, repeat_columns } | page_group { pages }`); `validate` (`report.slot_invalid` naming the block for unknown values, empty or >10 000 ID lists, missing pages, row rule on a non-table block) and `order` (sort key, then ascending stable ID: a total order). All identifiers untrusted, compared only. Acceptance: AC-1..AC-4 in `spec/rpt/model/tmpl/slot/NODE.md`. Constraints: CONSTITUTION C6, §8; no std HashMap/HashSet.

## Open questions

- `field_table` and column repetition were added for the fixed form templates (`RPT.FORMS.LAYOUT.*`).

## Children

_Leaf: none._
