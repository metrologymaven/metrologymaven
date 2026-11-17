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
satisfies: [REQ-RPT.MODEL-001, REQ-RPT.MODEL-015]
covers: [GOM.RPT.01]
depends_on: [CORE.JRN, CORE.STO, CORE.REG]
side_effect: W2
determinism: D0
cli: [fid report create-report, fid report add-page, fid report remove-page, fid report add-block, fid report remove-block, fid report show-report]
mcp: [report_create_report, report_add_page, report_remove_page, report_add_block, report_remove_block, report_show_report]
journal: [report.create_report@1, report.add_page@1, report.remove_page@1, report.add_block@1, report.remove_block@1]
updated: 2026-11-16
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
- **Crate:** `fid-report`.

## Test fixtures

- `fixtures/rpt/model/empty_project`: a project with one part, revision and plan and no results; generated by the CORE project test builder.
- `fixtures/rpt/model/hostile_strings`: a JSON list of hostile strings written for this test (prompt-injection text, bidi controls, a 5000-byte string, an embedded NUL). Licence CC-BY-4.0, authored in-project.
- Seeded random sequence (seed 42) generated in the test itself.

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

## Children

_Leaf: none._
