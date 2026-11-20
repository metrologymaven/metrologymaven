---
id: RPT.MODEL.BIND.RESOLVE
title: Block binding and resolution of stable IDs against a journal head
level: 4
parent: RPT.MODEL.BIND
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 298
satisfies: [REQ-RPT.MODEL.BIND-001, REQ-RPT.MODEL.BIND-004, REQ-RPT.MODEL.BIND-005, REQ-RPT.MODEL.BIND-010]
covers: [GOM.RPT.07]
depends_on: [RPT.MODEL.DOC, RPT.MODEL.TMPL.SLOT, CORE.JRN, CORE.PRJ.CONT, GDT.CHAR.RECORD]
side_effect: W2
determinism: D0
cli: [fid report bind-block]
mcp: [report_bind_block]
journal: [report.bind_block@1]
updated: 2026-11-17
---

# RPT.MODEL.BIND.RESOLVE: Block binding and resolution of stable IDs against a journal head

## Purpose

Two halves of the same contract. `report.bind_block@1` stores in a block only the stable IDs it shows (ResultRecords, characteristics, saved views, statistics series), never copied values. The resolver then looks every stored ID up as of an explicit journal head and hands the block kind a typed `Resolved` view, refusing IDs that are not journaled by that head or that belong to another project. Block kinds plug in through the `BlockResolver` trait defined here, so the engine never needs to know each kind.

## Scope

**In:** `Binding` type (a list of typed stable-ID references) stored in a block's payload envelope; `report.bind_block@1` (W2); `ResolveContext` (project, journal head, stage and run selection, optional `FieldSource`s); `BlockResolver` trait and its registry; `FieldSource` trait through which form data (FAI, PPAP) supplies field values and table rows; ID lookup against the journal; errors `report.unjournaled_record` and `report.cross_project_binding`.

**Out:** repetition and multi-stage expansion (`RPT.MODEL.BIND.EXPAND`); the resolved document, digest and generation commands (`RPT.MODEL.BIND.GEN`); formatting (`RPT.MODEL.FMT`); per-kind payloads and their resolver implementations (`RPT.MODEL.BLOCKS.*`, `RPT.FORMS.LAYOUT.MAP`, `RPT.BALLOON.BLOCK`); slot rule language (`RPT.MODEL.TMPL.SLOT`).

## Requirements

- **REQ-RPT.MODEL.BIND.RESOLVE-001** When `report.bind_block@1` runs, the system shall store in the block only typed stable-ID references (`result`, `characteristic`, `named_view`, `series`, `field`) and shall reject with error `report.binding_has_value` any reference carrying a key besides its kind and ID, and any payload holding, outside its ID fields (the top-level `binding` and every object tagged `binding@1` or `value_binding@1`), a JSON number (reason `number`), a bit pattern (reason `bit_pattern`: `0x` or `0X` followed by one or more hex digits) or value text (reason `number_text`: optional blanks; an optional sign `+`, `-` or U+2212; a mantissa of ASCII digits `d+`, `d+.`, `d+.d+` or `.d+` with `.` or `,` as separator; an optional exponent `e` or `E`, optional sign, `d+`; then optionally blanks and a unit whose first character is an ASCII letter or one of `µ μ ° % ' " ′ ″` and whose other characters are those, ASCII digits or `/ ^ · . ² ³`; then optional blanks, where blanks are U+0020, U+0009, U+00A0 and U+202F); resolution applies the same rule to the stored payload and to the block kind's layout.
- **REQ-RPT.MODEL.BIND.RESOLVE-002** When a binding is resolved, the resolver shall look up every referenced ResultRecord and characteristic as of the journal head given in the `ResolveContext`, never as of the current head.
- **REQ-RPT.MODEL.BIND.RESOLVE-003** If a referenced ResultRecord is not committed in the journal up to the given head, then resolution shall fail with error `report.unjournaled_record` naming the block ID and record ID, and nothing shall be journaled.
- **REQ-RPT.MODEL.BIND.RESOLVE-004** If a binding names a ResultRecord or characteristic of another project, then resolution shall fail with error `report.cross_project_binding` naming the block and the foreign project ID.
- **REQ-RPT.MODEL.BIND.RESOLVE-005** Each block kind shall be resolved through the `BlockResolver` registered for its kind ID, and if none is registered then resolution shall fail with error `report.no_resolver` naming the kind.
- **REQ-RPT.MODEL.BIND.RESOLVE-006** If `report.bind_block@1` names a reference that does not exist at the current head, then it shall fail with error `report.not_found` and journal nothing.
- **REQ-RPT.MODEL.BIND.RESOLVE-007** `report.bind_block@1` shall write each bound value as a `value_binding@1` (record ID, record digest, field pointer, statement digest; shape pinned in `RPT.MODEL.DOC`), and resolution shall apply REQ-RPT.MODEL.BIND-010: recompute both digests from the stored record and the statement the record references for that field (`report.binding_digest_mismatch`), and refuse a `field` that is not one of the record's value fields (`report.binding_field_not_value`).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage` and a `table` block, when `fid report bind-block --report <r> --block <b> --results r1,r2 --json` runs, then every declared value field of the stored payload holds a `value_binding@1` for r1 or r2 (record ID, record digest, value-field pointer, statement digest), the payload holds no number and no value text, `RPT.MODEL.DOC` accepts it, the parameters and the result validate against the declared schemas, and one `report.bind_block@1` event is journaled.
- **AC-2** (error path) Given a payload that carries `"value": 12.003` next to a bound record, when bound, then it fails with `report.binding_has_value`.
- **AC-3** Given AC-1 and head H0 before record r2 was committed, when resolved at H0, then it fails with `report.unjournaled_record` naming r2; resolved at the current head, it succeeds.
- **AC-4** (error path) Given fixture `fixtures/rpt/model/two_projects`, when a block binds a record of project B inside project A, then resolution fails with `report.cross_project_binding`.
- **AC-5** (error path) Given a block of a kind with no registered resolver, when resolved, then it fails with `report.no_resolver`.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.
- **AC-7** (error path) Given the block from AC-1 with the `record_digest` (and, separately, the `statement_digest`) of r1's binding replaced by another valid `sha256:` digest, when resolved, then it fails with `report.binding_digest_mismatch` naming the binding and produces no value text.
- **AC-8** (error path) Given a binding whose `field` points into r1's `fid.gdt-result/1` record somewhere other than its value field `/value` (`/value/value`, `/value/unit`, `/uncertainty/expanded/U`, `/decision`, the whole record `""`, or the absent `/nominal` and `/limits/upper`: GDT records carry no nominal or limits, which belong to the characteristic), when resolved, then it fails with `report.binding_field_not_value`.

## Design

- **Registry command:** `report.bind_block@1(report: ReportId, block: BlockId, refs: Vec<Ref>) -> Binding` (W2).
- **Data structures:** `Ref { kind: Result | Characteristic | NamedView | Series | Field, id }`; `Binding { refs: Vec<Ref> }` (order kept); `ResolveContext { project, head: JournalHash, stages: Vec<StageId>, runs: Vec<RunId>, sources: Vec<&dyn FieldSource> }`; `trait BlockResolver { fn kind(&self) -> BlockKindId; fn place(&self, payload: &CanonicalJson, binding: &Binding, values: &[ValueBinding]) -> Result<CValue, PayloadError>; fn resolve(&self, view: &Resolved, ctx: &ResolveContext, lookup: &dyn Lookup) -> Result<CValue, ResolveError>; }` (amended at build: the engine builds `Resolved` and `ResolvedBlock` itself, and a kind returns only its placement or its layout, never a view or a value); `trait FieldSource { fn field(&self, id: &FieldId) -> Option<FieldValue>; fn rows(&self, table: &TableId) -> Vec<RowSource>; }`; `trait Lookup { fn result(&self, id) -> Result<ResultView, LookupFault>; fn characteristic(&self, id) -> Result<EntityView, LookupFault>; fn named_view(..); fn series(..); fn statement(&self, digest) -> Result<StatementBytes, LookupFault>; }` backed by journal reads at `head` (amended at build: `ResultView` holds the record's ID, digest, typed value fields and evidence facts, never its bytes).
- **Errors:** `report.binding_has_value`, `report.unjournaled_record`, `report.cross_project_binding`, `report.no_resolver`, `report.not_found`.
- **Determinism:** D0; lookups are pure reads at a fixed head.
- **Performance budget:** 10 000 lookups in under 100 ms on reference hardware [unverified until measured].
- **Crate:** `fid-report` (module `bind`).
- **Pinned at build** (each rule has a test in `crates/fid-report/tests/bind.rs`):
  - *Stored binding.* A bound block's payload holds `binding` = `{"refs": [{"id", "kind"}], "schema": "binding@1"}` (kinds `result`, `characteristic`, `named_view`, `series`, `field`; IDs are canonical UUIDs, field IDs `^[a-z][a-z0-9_.]{0,63}$`; order kept, repeats refused, at most 4096). A reference has exactly `id` and `kind`: any other key is `report.binding_has_value` naming it, and so is a value outside the ID fields anywhere in the payload the command stores or resolution reads, or in a kind's layout (REQ-001). Kinds that bind accept the reserved key `binding`.
  - *Values.* For each `result` reference the command writes one `value_binding@1` per value field of the stored record; the kind places them through `BlockResolver::place` (added to the Design's trait: the engine cannot know a kind's payload layout). ResultRecords are read in the `fid.gdt-result/1` wire form (`GDT.CHAR.RECORD`), whose one value field is `/value` with its statement at `/uncertainty/statement`; a value field without a statement (UEC-0) cannot be bound (`report.value_binding_invalid`, `no_statement`), since `value_binding@1` requires `statement_digest`.
  - *Resolution order.* resolver (`report.no_resolver`), the no-value rule on the payload, head (`report.not_found`), stored binding, references in order (for results and characteristics project first, so a foreign entity is `report.cross_project_binding` holding only the block and the foreign project; saved views and series are not owner-checked, per the Open question; then journal position, `report.unjournaled_record` for any journaled kind; then kind, `report.not_found`), then each declared value field (record referenced by the binding, record digest of the bytes stored at the head, value field, statement digest, in that order), then the kind's layout and the no-value rule on it. The kind gets a typed `Resolved` view built by the engine (private fields, no public constructor); its values are `Quantity` (`SiValue` plus unit) with the record's `Evidence`: evaluation basis, evidence class and method, claimed / declared-input / uncharacterized-input facts and the decision (source, rule, outcome, reason), read from the journaled record and checked against the decision's own evidence basis, with `looks_measured()` false for claimed, declared-input, uncharacterized-input, UEC-0, `RESULTS_ONLY` and `DIRECT_READING` values (REQ-RPT.MODEL.BIND-010). Record bytes never reach a kind.
  - *Journal seam.* `JournalReads` (head position, current head, `entity(id, seq)` and `statement(project, digest, seq)`, each returning what the store held as of that journal position: the version committed at or before it, the fact that it was first committed later with its owner and kind, or nothing) is implemented by the store adapter of the bus; `MemoryJournal` in memory, which keeps every version of an ID and keys statements by (project, digest), so two projects may store byte-identical statements. `report.bind_block@1` reads at the current head; resolution only at the head it is given.
  - *Command.* Declared with `#[fid_command]` (W2, D0, `idempotency: none`, `undo: head_move`, MCP exposed). One event `report.bind_block@1 {type, report, block, payload, report_digest}` replayed by `RPT.MODEL.DOC`'s `replay_exact`. Branch precedence (`report.policy`): any MCP call, whatever its principal, and any agent principal write only on an agent branch `agent/<client>/<n>`, so a human principal over MCP on an agent branch is allowed; otherwise a human never writes on an agent branch; scripts follow the bus's routing. Whether the agent branch is the caller's own is deferred to CORE.BUS.ROUTE. CLI: `fid report bind-block --report <r> --block <b> [--results|--characteristics|--views|--series|--fields a,b] --json` (`--report` added: block IDs are unique per report only).

## Test fixtures

`fixtures/rpt/model/bracket_3stage` (one part, 3 stages, 50 characteristics with results; CORE project test builder) and `fixtures/rpt/model/two_projects`. Synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, implement block binding and ID resolution.
>
> **Command:** `report.bind_block@1(report, block, refs)` (W2, one journal event; CLI `fid report bind-block --json`). Payloads store typed stable-ID references only (`result`, `characteristic`, `named_view`, `series`, `field`); a payload carrying a value for a bound reference -> `report.binding_has_value`; unknown reference -> `report.not_found`.
>
> **Resolver:** `ResolveContext { project, head, stages, runs, sources }`; `BlockResolver` trait plus registry keyed by block kind; `FieldSource` trait (`field(id)`, `rows(table)`) for form data providers (`report.no_resolver` when missing); `Lookup` over journal reads at exactly `head`. Records not committed by `head` -> `report.unjournaled_record`; records or characteristics of another project -> `report.cross_project_binding`. Failures journal nothing.
>
> **Acceptance:** AC-1..AC-8 in `spec/rpt/model/bind/resolve/NODE.md`.
>
> **Constraints:** CONSTITUTION C1 (no value is copied or computed here), C6, §5 stable IDs, §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no std HashMap/HashSet.

## Open questions

- Cross-project data enters reports only as statistics series (`series` references resolved by the chart block); this leaf therefore checks project ownership only for results and characteristics.
- **Against RPT.MODEL.DOC (UEC-0):** `value_binding@1` requires `statement_digest`, but a UEC-0 result has no uncertainty statement, so UEC-0 results cannot be bound or reported yet (`report.value_binding_invalid`, `no_statement`). DOC should decide how a UEC-0 value is bound (for example a nullable `statement_digest` that resolution checks against the record's `null`).
- **Transcription and gauge facts:** the `fid.gdt-result/1` wire form carries no transcription record or gauge, so `Evidence` holds the evaluation basis (`RESULTS_ONLY`, `DIRECT_READING`) but not those records; `RPT.MODEL.FMT` still needs them from their owners (IO.RES.TRANS.MODEL, the gauge register) to label such values.

## Children

_Leaf: none._
