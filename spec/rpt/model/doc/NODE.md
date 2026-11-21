---
id: RPT.MODEL.DOC
title: Report document model and storage
level: 3
parent: RPT.MODEL
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-report]
built_pr: 232
satisfies: [REQ-RPT.MODEL-001, REQ-RPT.MODEL-015]
covers: [GOM.RPT.01]
depends_on: [CORE.JRN, CORE.STO, CORE.REG]
side_effect: W2
determinism: D0
cli: [fid report create-report, fid report add-page, fid report remove-page, fid report add-block, fid report remove-block, fid report show-report]
mcp: [report_create_report, report_add_page, report_remove_page, report_add_block, report_remove_block, report_show_report]
journal: [report.create_report@1, report.add_page@1, report.remove_page@1, report.add_block@1, report.remove_block@1]
updated: 2026-11-21
---

# RPT.MODEL.DOC: Report document model and storage

## Purpose

The pure-data report document: a report is an ordered list of pages, and each page is an ordered list of typed content blocks. Every report, page and block has a stable ID that survives save, reload, regeneration and replay. This leaf defines the types, their canonical storage in the project, and the registry commands that create and remove reports, pages and blocks. Block payloads are opaque here: each block kind registers its own payload schema (owned by `RPT.MODEL.BLOCKS`, `RPT.BALLOON` and `RPT.FORMS`).

## Scope

**In:** `Report`, `Page`, `Block` and `BlockKind` types; the block-kind registry trait; stable IDs (UUIDv7, CONSTITUTION §5); canonical JSON storage and content digest; commands to create a report, add and remove pages, add and remove blocks, and show a report; untrusted typing of every user string.

**Out:** block payload schemas and their rendering (`RPT.MODEL.BLOCKS`); templates, slots and repetition (`RPT.MODEL.TMPL`); binding and generation (`RPT.MODEL.BIND`); block position, size and style on the page (`RPT.DESIGN`); number formatting (`RPT.MODEL.FMT`).

## Requirements

- **REQ-RPT.MODEL.DOC-001** When `report.create_report@1` runs, the system shall create a report with a new UUIDv7 report ID and one empty page with its own UUIDv7 page ID, and journal one event.
- **REQ-RPT.MODEL.DOC-002** When `report.add_block@1` runs, the system shall append a block with a new UUIDv7 block ID to the named page, or insert it after a named sibling block, and journal one event.
- **REQ-RPT.MODEL.DOC-003** When `report.add_page@1` runs, the system shall insert a new empty page after the named page (or at the end) and journal one event.
- **REQ-RPT.MODEL.DOC-004** When a block or page is removed, the system shall never reuse its ID within the same report.
- **REQ-RPT.MODEL.DOC-005** If `report.remove_page@1` targets a page that still holds blocks and `force` is false, then the system shall fail with error `report.page_not_empty` and journal nothing.
- **REQ-RPT.MODEL.DOC-006** If a command names a report, page or block ID that does not exist, then the system shall fail with error `report.not_found` naming the ID and journal nothing.
- **REQ-RPT.MODEL.DOC-007** If `report.add_block@1` names a block kind that is not registered, or a payload that fails the kind's schema check, then the system shall fail with error `report.unknown_block_kind` or `report.invalid_block_payload` and journal nothing.
- **REQ-RPT.MODEL.DOC-008** When a report is saved and reloaded, every report, page and block ID and the order of pages and blocks shall be unchanged, and the stored canonical JSON (RFC 8785) shall have the same SHA-256 digest.
- **REQ-RPT.MODEL.DOC-009** Every user-entered string in the report model (title, block captions, free text) shall be stored as an untrusted-content type and marked untrusted in every CLI `--json` and MCP output (CONSTITUTION C6).
- **REQ-RPT.MODEL.DOC-010** If a user-entered string is longer than 4096 bytes of UTF-8 or contains a NUL character, then the system shall reject it with error `report.text_invalid` and journal nothing.
- **REQ-RPT.MODEL.DOC-011** When the journal is replayed in exact mode, the replayed report shall have the same IDs as the original, because IDs are taken from the journal events rather than generated again.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/empty_project`, when `fid report create-report --title "Bracket FAI" --json` runs, then the result holds a UUIDv7 `report_id`, exactly one page with a UUIDv7 `page_id`, zero blocks, and the journal gains exactly one `report.create_report@1` event.
- **AC-2** Given the report from AC-1, when three `text` blocks are added to page 1, `add-page` runs, and one `image` block is added to page 2, then `fid report show-report --json` lists 2 pages with blocks in insertion order, and closing and reopening the project gives the same IDs, order and stored digest.
- **AC-3** (error path) Given the report from AC-2, when `fid report remove-page --page <page 1>` runs without `--force`, then it fails with `report.page_not_empty` and the journal length is unchanged.
- **AC-4** (error path) Given the report from AC-2, when `fid report add-block --kind no_such_kind` runs, then it fails with `report.unknown_block_kind` and journals nothing; when `remove-block` names a random UUID, it fails with `report.not_found`.
- **AC-5** (untrusted) Given fixture `fixtures/rpt/model/hostile_strings` (titles with prompt-like text, bidi controls and a 5000-byte string), when each is used as a title, then the first two are stored and shown with `"untrusted": true`, and the 5000-byte one fails with `report.text_invalid`.
- **AC-6** (ID non-reuse) Given a report, when 1000 blocks are added and removed in a seeded random order (seed 42), then no block ID is ever issued twice.
- **AC-7** (replay) Given the journal from AC-2, when replayed in exact mode, then the outcome is `IDENTICAL` and the replayed report digest equals the original.

## Design

- **Registry commands** (all W2 except `show_report`, which is R0):
  - `report.create_report@1(title: Text, project: ProjectRef) -> ReportSummary`
  - `report.add_page@1(report: ReportId, after: Option<PageId>) -> PageId`
  - `report.remove_page@1(report: ReportId, page: PageId, force: bool) -> ()`; with `force`, its blocks are removed too
  - `report.add_block@1(report: ReportId, page: PageId, after: Option<BlockId>, kind: BlockKindId, payload: CanonicalJson) -> BlockId`
  - `report.remove_block@1(report: ReportId, block: BlockId) -> ()`
  - `report.show_report@1(report: ReportId) -> Report`
- **Data structures:** `Report { id, schema_version: u32, title: Untrusted<Text>, pages: Vec<Page>, template_ref: Option<TemplateRef>, retired_ids: BTreeSet<Uuid> }`; `Page { id: PageId, blocks: Vec<Block> }`; `Block { id: BlockId, kind: BlockKindId, kind_version: u32, payload: CanonicalJson }`. `BlockKindId` is a short string such as `text`, `table`, `chart`, `view`, `image`, `header`, `footer`, `drawing`, `form_field`. Use `BTreeMap`/`BTreeSet` only (no std HashMap, for deterministic serialisation).
- **Block-kind registry:** trait `BlockKind { fn id(&self) -> BlockKindId; fn version(&self) -> u32; fn validate(&self, payload: &CanonicalJson) -> Result<(), PayloadError>; }`. This leaf ships only the trait, the registry, and a `text` stub kind for tests; real kinds are registered by other crates.
- **Storage:** each report is stored as one RFC 8785 canonical JSON document in the project store (CORE.STO), keyed by report ID, with its SHA-256 digest. IDs are generated by the command handler and written into the journal event, so replay reads them back instead of generating new ones.
- **Errors:** `report.not_found`, `report.page_not_empty`, `report.unknown_block_kind`, `report.invalid_block_payload`, `report.text_invalid`.
- **Determinism:** D0. No floating point; IDs are journaled values; serialisation is canonical.
- **Performance budget:** add or remove a block in a 500-page, 10 000-block report in under 20 ms on reference hardware [unverified until measured].
- **Crate:** `fid-report` (module `doc`).
- **Pinned at build** (fid-report `doc`; each rule has a test in `crates/fid-report/tests/doc.rs` or `tests/value.rs`):
  - *Stored form.* One RFC 8785 document per report, `schema_version` `"1"`: `{id, pages: [{blocks: [{id, kind, kind_version, payload}], id}], retired_ids, schema_version, template_ref, title}`. Integers (`schema_version`, `kind_version`, template `version`, `block_count`) are decimal strings, so the model holds no JSON numbers of its own; `retired_ids` is ascending; `template_ref` is `null` or `{builtin, digest, id, version}` (`builtin` a boolean, `digest` the `sha256:` digest of the applied template version, `id` its UUIDv7 template ID; widened by #391, RPT.MODEL.TMPL.VER). The reader is strict: duplicate keys, unknown or missing keys, a non-UUIDv7 ID, a repeated ID, unsorted arrays, bytes that are not the canonical form of what they describe, or a stored digest that does not match the bytes are `report.store_invalid`. Reading also re-checks every block against the kind registry: an unregistered kind or kind version is `report.unknown_block_kind`, a failing payload `report.invalid_block_payload`, and the value rule below applies (`report.value_literal_refused` or `report.value_binding_invalid`). `show_report` returns exactly the stored document.
  - *Untrusted text.* `Untrusted<Text>` has no `Display` or `Deref`; its machine form everywhere (storage, `--json`, MCP, events) is `{"text": <string>, "untrusted": true}`, and its schemas carry `x-untrusted: true`. The only human form is `escaped()`: backslash doubled; C0 and C1 controls and DEL, every general-category Cf code point and every Default_Ignorable_Code_Point (Unicode 16.0 tables copied verbatim from `crates/fid-bus/src/unicode16.rs`, with a test that the copies match), U+2028, U+2029 and U+FFFC written as `\u{XXXX}`. Payloads are opaque here, so every payload is wrapped as `{"untrusted": true, "value": <json>}`. The 4096-byte check runs before the NUL check.
  - *Values.* Declared value fields hold bindings, never values. DOC never formats, computes or trusts a value or its labels, and blocks carry no label set (the earlier block-level `ValueLabel` set and `required_labels` were removed: a block-level set cannot say which value it covers). `BlockKind::value_fields(&payload)` (required, no default) returns RFC 6901 pointers to every value field; a kind without values returns an empty list. Each declared field must hold a `value_binding@1`. A number, a string (number-like or not), a boolean, null, an array, an object without `schema`, or an object whose `schema` is not `value_binding@1` (a hand-typed `value_text@1` included) is refused with `report.value_literal_refused`; a missing field or a malformed binding with `report.value_binding_invalid` and the reason. Undeclared places are checked too: the whole payload is walked iteratively (within the payload bounds), objects in key order and arrays in index order, and an object whose `schema` is `value_text@1` anywhere is `report.value_literal_refused` (reason `value_text`), while an object whose `schema` is `value_binding@1` at a pointer that is not a declared value field is `report.value_binding_invalid` (reason `undeclared_binding`); the error names the RFC 6901 pointer of the first offence (cut at 128 bytes). The walk keeps only borrowed `(node, parent index, key or index)` entries (at most 65 536, never more than the payload has nodes) plus a stack of indices, matches declared fields by node address, and builds pointer text only for the one node it reports, so its memory is a constant times the payload size. Free text in undeclared places stays under REQ-RPT.MODEL.BLOCKS-009. The rule runs in `add_block`, in `Op::apply` (so in every replayed event) and when a stored report is read. A kind declaring a malformed pointer is `report.invalid_block_payload`.
  - *`value_binding@1`.* An object with exactly the keys `schema` (`"value_binding@1"`), `record` (canonical lowercase UUID of the bound record, not nil or max), `record_digest` and `statement_digest` (`sha256:` plus 64 lowercase hex: the stored record and its stored fid-kunc uncertainty statement), and `field` (an RFC 6901 pointer into the record, `""` or starting with `/`, `~` only as `~0` or `~1`, at most 256 bytes, no character `escaped()` would escape). DOC checks only this shape; `RPT.MODEL.BIND` resolves the binding against the stored record and statement and refuses a digest mismatch (REQ-RPT.MODEL.BIND-010), and `RPT.MODEL.FMT` alone produces value text, with its own per-value labels and side (REQ-RPT.MODEL.FMT-012, -013).
  - *IDs.* Parameters accept any canonical lowercase UUID (another spelling is `report.bad_params`), so a random UUID fails as `report.not_found` naming it. New IDs are UUIDv7 drawn through `fid-project`'s `ObservationSource::draw_v7`; a drawn ID that is the project ID, the report ID, a live ID or a retired ID fails closed with `report.id_collision` (no retry). Report and page IDs are distinct.
  - *Commands.* Results: create gives `{report_id, title, pages: [{page_id, block_count}], report_digest}`; add page gives `{page_id}`; add block gives `{block_id}`; removals give `{}`; show gives the report. `project` must equal the open project, else `report.not_found`. `add_block` checks report, page, `after` on that same page, kind, payload bounds, payload and value rule, in that order. A kind string that is not `^[a-z][a-z0-9_]{0,31}$` is `report.unknown_block_kind` and is not echoed. Removing the last page is allowed. Registry definitions: W2, D0, `idempotency: none`, `undo: head_move`, MCP `exposed`; show is R0, `natural`, `not_undoable`.
  - *Events.* One per W2 command, holding every ID it created and `report_digest` (the stored digest after the command): `create_report {report, page, project, title}`, `add_page {report, after, page}`, `remove_page {report, page, force}`, `add_block {report, page, after, block, kind, kind_version, payload}`, `remove_block {report, block}`, each with `type` and `report_digest`. Event bytes are read with duplicate keys refused (`report.store_invalid`).
  - *Exact replay.* `replay_exact(events, kinds, project)` applies each event with its own IDs and compares the replayed digest: an unreadable event or a missing kind or kind version is `NOT_REPLAYABLE`; a `create_report` event naming another project, a failing operation (the value rule included) or a different digest is `DIVERGENT`; otherwise `IDENTICAL`. It stops at the first event that is not identical.
  - *Storage seam.* Handlers hand one stored document and one event to a `ReportBackend`, which commits both or neither; the SQLite store adapter belongs to the bus (CORE.BUS.LINT forbids this crate reaching the SQLite driver). `MemoryBackend` is the in-memory backend.
  - *Bounds.* Text 4096 bytes; kind ID 32 bytes; payload 65 536 canonical bytes and depth 32; 4096 value fields per block and 256 bytes per pointer; 10 000 pages; 100 000 blocks; 262 144 IDs ever issued per report; 32 MiB per stored report; 256 kinds per registry; 1 000 000 events per replay call. Exceeding one is `report.limit_exceeded` (`report.invalid_block_payload` inside a payload).
  - *Extra error codes.* Besides the five above: `report.value_literal_refused`, `report.value_binding_invalid`, `report.bad_params`, `report.id_collision`, `report.id_unavailable`, `report.limit_exceeded`, `report.store_invalid`, `report.backend`.
  - *Test kinds.* Stub kinds `text` (`{"text": <Text>}`) and `image` (`{"blob": "sha256:..."}`), both version 1, behind the `test-kinds` feature; AC-2 needs `image` as well as `text`.

## Test fixtures

- `fixtures/rpt/model/empty_project`: a project with one part, revision and plan and no results; generated by the CORE project test builder (until it exists, hand-written by `fixtures/rpt/model/doc/make_fixtures.py`).
- `fixtures/rpt/model/hostile_strings`: a JSON list of hostile strings written for this test (prompt-injection text, bidi controls, a 5000-byte string, an embedded NUL). Licence CC-BY-4.0, authored in-project.
- Seeded random sequence (seed 42, splitmix64) generated in the test itself.
- `fixtures/rpt/model/doc/payload_cases.json`: hand-labelled whole payloads (value text or bindings in undeclared places) with the expected code, pointer and reason, written by `make_fixtures.py`.
- `fixtures/rpt/model/doc/binding_cases.json` and `binding-report-text.json`: hand-labelled value-field contents (well-formed bindings, literal value text, hand-typed numbers and number-like strings, malformed bindings) with the expected error code and reason, and the exact stored bytes of a report holding one binding, written by `make_fixtures.py`.
- `fixtures/rpt/model/doc/*-text.json` and `digests.txt`: hand-labelled canonical bytes and digests of the AC-1 summary, the AC-2 report and its journal, written by `make_fixtures.py` from the rules above with a deterministic ID source (draw k gives `unix_ms = 1763510400000 + k` and random bits `k + 1`).

## Build prompt

> In crate `fid-report`, implement the report document model and its registry commands.
>
> **Types:** `Report`, `Page`, `Block`, `BlockKindId`, `ReportId`/`PageId`/`BlockId` (UUIDv7 newtypes) and `Untrusted<Text>`. Use `BTreeMap`/`BTreeSet`, never std `HashMap`/`HashSet`. Serialise with RFC 8785 canonical JSON and compute SHA-256 digests.
>
> **Commands** (register through the command registry; `--json` on every CLI command; MCP names are derived by the registry): `report.create_report@1`, `report.add_page@1`, `report.remove_page@1 { force }`, `report.add_block@1`, `report.remove_block@1` (all side-effect class W2, journaled, one event each) and `report.show_report@1` (R0).
>
> **Rules:** IDs are UUIDv7, generated by the handler and recorded in the journal event; on replay, read them from the event. Never reuse a removed ID within a report (keep `retired_ids`). Every user string is `Untrusted<Text>`, at most 4096 UTF-8 bytes, no NUL; otherwise `report.text_invalid`. Errors are typed: `report.not_found`, `report.page_not_empty`, `report.unknown_block_kind`, `report.invalid_block_payload`, `report.text_invalid`; a failed command journals nothing. Provide a `BlockKind` trait (`id`, `version`, `validate`) and a registry; ship a `text` stub kind for tests only.
>
> **Acceptance tests:** AC-1 to AC-7 above, using fixtures `fixtures/rpt/model/empty_project` and `fixtures/rpt/model/hostile_strings`, including the exact-replay test that must report `IDENTICAL`.
>
> **Constraints:** no panic, no `unsafe`, no `unwrap`/`expect`/`todo`/`unimplemented`/`unreachable` (CONSTITUTION §8); all input is hostile and bounded; agents call the W2 commands on an agent branch only (enforced by the command bus, not here). Do not implement block payload schemas, templates, binding or layout. Spec: `spec/rpt/model/doc/NODE.md`.

## Open questions

- Page commands (`*-page`) extend the parent's original `*-report|*-template|*-block` patterns; the parent's patterns were widened accordingly.

## Change log

- 2026-11-21: the stored `template_ref` is `{builtin, digest, id, version}`, not `{id, version}`, matching what #391 (RPT.MODEL.TMPL.VER) built in `fid-report`: REQ-RPT.MODEL.TMPL.VER-004 needs the applied version's digest, and built-in and project versions are separate number spaces. No stored report held a non-null `template_ref` before #391. No REQ or AC changed.

## Children

_Leaf: none._
