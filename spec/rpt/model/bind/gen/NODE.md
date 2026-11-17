---
id: RPT.MODEL.BIND.GEN
title: Report generation, resolved document, provenance and digest
level: 4
parent: RPT.MODEL.BIND
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.MODEL.BIND-002, REQ-RPT.MODEL.BIND-003, REQ-RPT.MODEL.BIND-007, REQ-RPT.MODEL.BIND-008, REQ-RPT.MODEL.BIND-009]
covers: []
depends_on: [RPT.MODEL.BIND.RESOLVE, RPT.MODEL.BIND.EXPAND, RPT.MODEL.FMT, RPT.MODEL.TMPL.VER, CORE.JRN, CORE.STO]
uses_artifacts: [I18N.LOCALE]
side_effect: [R1, W2]
determinism: D0
cli: [fid report generate-report, fid report save-report]
mcp: [report_generate_report, report_save_report]
journal: [report.generate_report@1, report.save_report@1]
updated: 2026-11-17
---

# RPT.MODEL.BIND.GEN: Report generation, resolved document, provenance and digest

## Purpose

The generation entry point and the `ResolvedReport` contract that output, preview and forms consume. Given a report or template, a journal head and a locale, it expands slots, resolves every block, collects formatter IDs, writes a provenance header and computes the RFC 8785 + SHA-256 content digest. Generating into scratch is R1; storing the resolved document in the project is the separate W2 command `report.save_report@1`.

## Scope

**In:** `ResolvedReport`, `ResolvedPage`, `ResolvedBlock` envelope and `Provenance` types (versioned schema); `generate(doc, ctx)` library entry used by forms and preview; `report.generate_report@1` (R1) and `report.save_report@1` (W2); content digest; warnings list; the performance budget.

**Out:** ID lookup (`RPT.MODEL.BIND.RESOLVE`); expansion (`RPT.MODEL.BIND.EXPAND`); value text (`RPT.MODEL.FMT`); per-kind resolved payloads (block kinds); rendering to files (`RPT.OUTPUT`); snapshot bytes (stored by digest, fetched by output).

## Requirements

- **REQ-RPT.MODEL.BIND.GEN-001** When generation runs, the system shall expand the document at the given journal head, resolve every block through its registered resolver and produce a `ResolvedReport` whose content digest is the SHA-256 of its RFC 8785 canonical JSON excluding the digest field.
- **REQ-RPT.MODEL.BIND.GEN-002** When a report is generated twice from the same journal head, template ID and version, formatter versions, locale, locale data release and unit override, the two resolved reports shall be byte-identical in canonical form.
- **REQ-RPT.MODEL.BIND.GEN-003** The `ResolvedReport` provenance shall record the journal head hash, template ID, version and digest (if any), kernel version, the sorted set of formatter IDs and versions used, the locale, the locale data release and the unit override.
- **REQ-RPT.MODEL.BIND.GEN-004** `report.generate_report@1` shall be side-effect class R1 and write nothing to the Record Journal; its resolved document shall live only in scratch storage.
- **REQ-RPT.MODEL.BIND.GEN-005** When `report.save_report@1` runs with a resolved-report digest from scratch, the system shall store the canonical bytes in the content-addressed store, link them to the report and journal one W2 event holding the digest; if the digest is not in scratch it shall fail with error `report.resolved_not_found`.
- **REQ-RPT.MODEL.BIND.GEN-006** The resolved document shall reference snapshot and image bytes by content digest only and shall not embed them.
- **REQ-RPT.MODEL.BIND.GEN-007** Non-fatal conditions reported by resolvers (for example `report.view_missing`, `report.display_tie`) shall be collected into a warnings list sorted by block ID and code, which is part of the digested content.
- **REQ-RPT.MODEL.BIND.GEN-008** Generation of a 500-characteristic, three-stage report shall complete in under 2 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/model/bracket_3stage` and the starter multi-stage template, when `fid report generate-report --template <t> --head <h> --json` runs twice, then both digests are identical, the provenance lists head, template ID/version/digest, kernel version, formatter IDs and locale `en`, and the Record Journal length is unchanged.
- **AC-2** Given AC-1, when generated with locale `de`, then the digest differs from AC-1, the provenance names `de` and the locale data release, and no stored value or ResultRecord digest changes.
- **AC-3** Given AC-1, when `fid report save-report --report <r> --resolved <digest>` runs, then the bytes in the content-addressed store hash to that digest and one `report.save_report@1` event is journaled; with an unknown digest it fails with `report.resolved_not_found`.
- **AC-4** Given a report with a view block, when generated, then the resolved block holds the snapshot digest and no image bytes.
- **AC-5** (golden) Given fixture `fixtures/rpt/model/bracket_3stage`, when generated, then the canonical JSON equals `fixtures/rpt/model/golden/bracket_3stage.resolved.json` byte for byte.
- **AC-6** (performance) Given fixture `fixtures/rpt/model/perf_500x3`, when generated on reference hardware, then it completes in under 2 s [unverified until measured].
- **AC-7** (replay) Given a journal containing AC-3, when replayed in exact mode, then the outcome is `IDENTICAL` and the saved digest is reproduced.

## Design

- **Registry commands:**
  - `report.generate_report@1(source: ReportId | TemplateRef, head: Option<JournalHash>, locale: Option<LocaleId>, units: Option<UnitOverride>, stages: Vec<StageId>, runs: Vec<RunId>) -> ResolvedReport` (R1); `units` (authored, or SI) is passed to the formatter layer as its unit override and recorded in the provenance
  - `report.save_report@1(report: ReportId, resolved: Sha256) -> SaveReceipt` (W2)
- **Data structures:** `ResolvedReport { schema_version, provenance: Provenance, pages: Vec<ResolvedPage>, warnings: Vec<Warning>, digest: Sha256 }`; `Provenance { head, template: Option<TemplateRef>, kernel_version, formatters: Vec<(FormatterId, Version)>, locale, locale_release, units }`; `ResolvedBlock { id, kind, kind_version, frame: Option<BlockFrame>, payload: CanonicalJson }`.
- **Algorithm:** default head = current head (recorded explicitly); `expand` -> per block `BlockResolver::resolve` with a formatting context (locale rules read from the `I18N.LOCALE` data artifact) -> collect formatter IDs and warnings -> canonicalise -> digest -> put in scratch keyed by digest.
- **Errors:** errors from resolve and expand, `report.resolved_not_found`, `report.not_found`.
- **Determinism:** D0; output is a pure function of the inputs listed in REQ-002.
- **Performance budget:** REQ-008 [unverified until measured].
- **Crate:** `fid-report`.

## Test fixtures

`fixtures/rpt/model/bracket_3stage` with its golden resolved JSON; `fixtures/rpt/model/perf_500x3` (generated by the CORE project test builder with seed 5). Synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-report`, implement report generation and the `ResolvedReport` contract.
>
> **Commands:** `report.generate_report@1(source, head?, locale?, stages, runs)` (R1, nothing in the Record Journal, result in scratch keyed by digest) and `report.save_report@1(report, resolved_digest)` (W2; store canonical bytes in the content-addressed store, one journal event; unknown digest -> `report.resolved_not_found`). CLI `fid report generate-report | save-report --json`.
>
> **Contract:** `ResolvedReport { schema_version, provenance { head, template (id, version, digest), kernel_version, formatters (sorted), locale, locale_release }, pages, warnings (sorted by block ID, code), digest }`. Digest = SHA-256 of RFC 8785 canonical JSON without the digest field. Images and snapshots by digest only. Generation = expand (EXPAND leaf) then resolve each block through its registered `BlockResolver` (RESOLVE leaf) with value text from the report formatter layer and locale rules from the pinned locale data file.
>
> **Acceptance:** AC-1..AC-7 in `spec/rpt/model/bind/gen/NODE.md`, including the golden resolved JSON and the 2 s budget.
>
> **Constraints:** CONSTITUTION C1 (no value computed here), C4/D-031 (R1 not in the Record Journal), §8 (no panic, no `unsafe`, deny `unwrap`/`expect`); no clock reads, no std HashMap/HashSet.

## Open questions

- The locale is part of the digest, so a German and an English rendering of the same journal are different resolved reports; ResultRecord digests and decisions never change (REQ-I18N.LOCALE-007).

## Children

_Leaf: none._
