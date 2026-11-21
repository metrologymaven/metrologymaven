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
built_in: [fid-report]
built_pr: 367
satisfies: [REQ-RPT.MODEL.BIND-006]
covers: [GOM.RPT.07]
depends_on: [RPT.MODEL.TMPL.SLOT, RPT.MODEL.BIND.RESOLVE, CORE.PRJ.STAGE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- **Pinned at build** (module `bind::expand`; each rule has a test in `crates/fid-report/tests/bind_expand.rs`; the model, templates and expected expansions come from the independent reference `fixtures/rpt/model/bind/expand/make_fixtures.py`, checked in CI with `--check`):
  - *API.* `expand(template, ctx: &ResolveContext, model: &dyn ProjectModel, journal: &dyn JournalReads, query: Option<&dyn QueryProvider>) -> Result<Vec<ExpandedBlock>, ExpandError>`. It reads only and journals nothing. It takes a `Template` (slots checked by `RPT.MODEL.TMPL.SLOT`'s `validate_template`); reports carry bindings, not slots, so there is nothing to expand in a report. Two parameters are added to the Design's signature. `ProjectModel` is the project as of `ctx.head`: characteristics (name, QIF type token, balloon, element, tags), elements, stages in project order (removed ones included), runs with start time, and which result sits in which stage. Like `JournalReads`, it is implemented by the bus's store adapter, with `MemoryModel` in memory. `journal` checks every selected characteristic at the head (`report.cross_project_binding`, `report.unjournaled_record`, `report.not_found`) and supplies the decisions the outcome filter reads. The filter never takes an outcome from the model.
  - *Output.* `ExpandedBlock { id, source_block, page, source_page, page_item, role, item, columns, binding, cells }`, with `role` one of `copy`, `table`, `row` or `column_group`. It holds stable-ID references only (`binding@1`), never a value. Its canonical form is `{"blocks": [...], "schema": "expansion@1"}` (RFC 8785).
  - *Selectors.* A listed ID the model does not hold is `report.not_found`. `stages` selects live stages only. `tags` selects characteristics, matched by bytes. `saved_query` answers may hold only characteristic references (`report.query_result_invalid` otherwise, an error added at build). `field_table` takes the rows of the first source that has any; row `i` has the stable ID `<table>/<i as ten digits>`. A characteristic type is compared as a token; membership in the QIF type list waits for `GDT.CHAR.MODEL.TYPES`.
  - *Outcome filter.* An item is kept when one of its results in the stage columns (for a stage, its own results; for a run, its stages' results; for an element, its characteristics' results) has an outcome in the filter. Field rows have no outcome.
  - *Order.* `tmpl::slot::order`, with these sort values: characteristic ID, name and element ID as text; balloon, stage position and run start time as integers; `Missing` otherwise. A `stages` or `runs` slot whose sort is `null` uses the REQ-003 order: stages by project position, then run start time, then run ID; runs by their first live stage's position, then start time, then ID.
  - *Stage columns (REQ-003).* `ctx.stages` if given (each must be live, else `report.not_found`), otherwise every live stage. If `ctx.runs` is given, only stages of those runs are kept. The result is in REQ-003 order. A `row` slot emits a `table` block carrying the columns, then one `row` per item. A characteristic row's binding is the characteristic followed by its result in each column; `cells` is aligned with the columns and holds null where a stage has no result. A `column` slot emits one `column_group` per run of at most `max_columns_per_group` items (one empty group when there are none).
  - *Page groups.* The group's pages, in document order, are emitted once per item at the place of the group's first page. A page in two groups is `report.slot_invalid` (`page_group_overlap`). So is a `page_group` slot on a page of another group (`nested_page_group`). No template recursion is possible. Slots on copied pages are evaluated unscoped (see Open questions).
  - *IDs.* UUIDv5, with the UTF-8 of the item's stable ID as the name. A page copy is `v5(template page, item)`. A block on a copy has base `v5(template block, page item)`; elsewhere its base is the template block ID. A row is `v5(base, item)`; a column group is `v5(base, first item)`.
  - *Bounds.* Per report: 100 000 rows; 100 000 columns (added at build: repeated columns plus each row table's stage columns); 1 000 000 cells, that is rows times stage columns (added at build) [unverified limit]; and 5 000 pages. Pages outside groups count once each and are checked first; their error names the template's first block. A slot's selected items are counted before any of them is looked up, filtered or sorted. The one exception is `saved_query`: its provider is called to learn the count, and its answer is capped at 1 000 000 entries. Crossing a bound is `report.expansion_too_large { block, limit: rows | columns | cells | pages }`. Each slot is evaluated once; page copies reuse its items and count them again. Each model list is capped at 1 000 000 distinct IDs and each characteristic at 256 tags (`report.store_invalid`). Indexes for stage→results, run→results, element→characteristics, characteristic→results and run→first stage are built once per expansion, and the stage columns are also held as a set. The outcome filter walks each item's own results and tests column membership, so its work is bounded by the result count (never characteristics times columns), and no step rescans the model per item. Row bindings walk the stage columns once per row, which the cell bound limits.

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
- **AC-1 and the starter template:** `RPT.MODEL.TMPL.STARTER` is not built. AC-1 runs against the fixture template `fixtures/rpt/model/bind/expand/templates/multistage.json`, shaped as STARTER's Design describes (row slot over characteristics, stage columns). Re-run it against `tmpl:multistage-table` once STARTER lands. STARTER's Design names "a column slot over stages" for that table, but a block has one slot. Here the stage columns of a row table come from the context's stage selection (REQ-003), and a separate `column` slot repeats columns over stages or runs. STARTER should use the row form.
- **Scoping inside page groups:** a slot on a page inside a page group is evaluated over the whole project, not restricted to the page's item (for example, one element's characteristics). Scoping needs a rule in `RPT.MODEL.TMPL.SLOT`.
- **ProjectModel adapter:** the bus's store adapter must implement `ProjectModel` from the project state (`CORE.PRJ.TREE`, `CORE.PRJ.STAGE`) and the Record Journal. Only `MemoryModel` exists today.

## Children

_Leaf: none._
