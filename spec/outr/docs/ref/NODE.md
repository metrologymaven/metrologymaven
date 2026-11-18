---
id: OUTR.DOCS.REF
title: Generated command reference pages
level: 3
parent: OUTR.DOCS
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-outreach]
satisfies: [REQ-OUTR.DOCS-001, REQ-OUTR.DOCS-002, REQ-OUTR.DOCS-003]
covers: []
depends_on: []
uses_artifacts: [AUTO.SURF.DOCS, CORE.REG.MAN]
side_effect: [W1, R0]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.DOCS.REF: Generated command reference pages

## Purpose

Renders the CLI, MCP and Python reference pages of each release from the structured `docs-ref-v1` records that the registry's docs emitter (AUTO.SURF.DOCS) writes for that release's manifest, places them at stable URLs, and checks that the published pages are exactly what a fresh rendering produces: one page of each kind per command, none extra, no hand edits. The pages are the registry (CONSTITUTION C3); this leaf adds layout only, never prose.

## Scope

**In:** the `ref-render` and `ref-check` subcommands; the page layout of the three renderings of one record; the URL scheme; the completeness and drift checks; the per-release surface list that says whether Python bindings ship.
**Out:** the registry manifest (`CORE.REG.MAN`); the reference records and their content, including help text and reason codes (`AUTO.SURF.DOCS`); the Python bindings themselves (`AUTO.PY`, not read by this leaf); the frame, editions and bundles (`OUTR.DOCS.FRAME`); the search index (`OUTR.DOCS.SEARCH`).

## Requirements

- **REQ-OUTR.DOCS.REF-001** For each release, `ref-render` shall write one CLI page, one MCP page and one Python page for every command major that has a `docs-ref-v1` record whose `manifest_digest` equals the release's registry manifest digest, and the docs build shall fail if any record is missing for a command in the manifest.
- **REQ-OUTR.DOCS.REF-002** If the reference tree contains a page for a command ID that has no record for the release's manifest, then `ref-check` shall fail and name the page.
- **REQ-OUTR.DOCS.REF-003** If a published reference page differs byte-for-byte from a fresh `ref-render` of the same records, then `ref-check` shall fail and name the page.
- **REQ-OUTR.DOCS.REF-004** Each reference page shall show the command ID and major, side-effect class, determinism class, `human_only` flag and MCP projection as given in the record.
- **REQ-OUTR.DOCS.REF-005** When a record's `mcp` surface is `available: false`, the MCP page shall state that the command is not available on MCP and give the record's reason code in words (never on MCP for this class, request-only, or human-only) instead of a tool schema.
- **REQ-OUTR.DOCS.REF-006** Each page shall be written at `<release>/ref/<cli|mcp|python>/<command_id>@<major>.html`, a path derived only from the release tag and the command ID.
- **REQ-OUTR.DOCS.REF-007** When the release's surface list does not include `python`, every Python page shall state that Python bindings are not part of this release and shall show no function signature.
- **REQ-OUTR.DOCS.REF-008** The rendering shall add no text other than fixed layout labels; every other word on a page shall come from the record.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/docs/ref/min` (the records AUTO.SURF.DOCS emits for `fixtures/manifest/min-classes.json`, a `surfaces.toml` with `cli` and `mcp`), when `fid-outreach ref-render --records min/records --manifest min-classes.json --surfaces surfaces.toml --release v0.1.0 --out ref` runs, then `ref/v0.1.0/ref/` holds three pages per command at the REQ-006 paths.
- **AC-2** Given AC-1 output, when a test reads the MCP page of the M1, S1 and A0 commands, then each states "not available on MCP" with reason "never on MCP for this class"; the W3, X1 and D1 pages state request-only; the `human_only` W2 page states human-only.
- **AC-3** Given AC-1 output, when a test reads any Python page, then it states that Python bindings are not part of this release and contains no signature; with `python` added to `surfaces.toml`, the page shows the record's Python name instead.
- **AC-4** Given AC-1 output, when `fid-outreach ref-check --records min/records --manifest min-classes.json --surfaces surfaces.toml --ref ref/v0.1.0/ref` runs, then it exits 0.
- **AC-5** (drift) Given fixture `ref/edited` (one CLI page with a changed word), when `ref-check` runs, then it exits 1 with `page_drift` naming the page.
- **AC-6** (extra and missing) Given fixture `ref/extra` (a page for `demo.ghost@1`) and fixture `ref/missing-record` (manifest command with no record), when `ref-check` runs, then it reports `page_without_command` and `record_missing` respectively.
- **AC-7** (stale records) Given records whose `manifest_digest` differs from the manifest's digest, when `ref-render` runs, then it exits 1 with `manifest_digest_mismatch` and writes nothing.
- **AC-8** (determinism) Given AC-1 run twice, then the two trees are byte-identical.

## Design

- **Commands:** `fid-outreach ref-render --records <dir> --manifest <file> --surfaces <file> --release <tag> --out <dir>` (nearest class W1: writes pages locally) and `fid-outreach ref-check --records <dir> --manifest <file> --surfaces <file> --ref <dir> [--json]` (R0). Not registry commands.
- **Inputs read:** from the manifest only its SHA-256 digest and `commands[].id`/major; from each record (`docs-ref-v1`): `manifest_digest`, `command_id`, `major`, `summary`, `help`, `params`, `result`, `side_effect`, `determinism`, `human_only`, `surfaces.{cli,mcp,python}.{available,name,projection,reason}`; `surfaces.toml` lists the surfaces the release ships (from release metadata).
- **Page layout:** fixed labels (Command, Side-effect class, Determinism, Human only, MCP, Parameters, Result); help text inserted verbatim and HTML-escaped; CLI page shows the CLI path and parameters; MCP page shows the tool name and input schema or the unavailability statement; Python page shows `fiducial.<domain>.<function>` or the not-in-release statement. Output is HTML fragments that `docs-build` (OUTR.DOCS.FRAME.NAV) wraps in the frame.
- **Algorithm:** verify digests; build a sorted map command -> record; render three pages each; for `ref-check`, render in memory and compare with the tree, collecting `record_missing`, `page_missing`, `page_without_command`, `page_drift`.
- **Errors:** fatal `ManifestUnreadable`, `RecordParse(file)`, `manifest_digest_mismatch`; findings as above.
- **Determinism:** D0.
- **Performance budget:** 5,000 command majors rendered and checked in under 10 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach`, module `ref_pages`.

## Test fixtures

The AUTO.SURF.DOCS golden records for `fixtures/manifest/min-classes.json` (copied into `fixtures/outr/docs/ref/min/`), plus hand-made variants `edited`, `extra`, `missing-record` and a stale-digest set. CC-BY-4.0.

## Build prompt

> Build two subcommands in the Rust outreach tooling crate `fid-outreach` (tooling only; it reads files and links no product crate).
>
> Input contract: a directory of `docs-ref-v1` JSON records (one per command major, `ref/<command_id>@<major>.json`, written by the registry docs emitter) with fields `manifest_digest`, `command_id`, `major`, `summary`, `help`, `params[] {name, ty, quantity_kind, unit, default}`, `result`, `side_effect`, `determinism`, `human_only`, `surfaces.{cli,mcp,python}.{available, name, projection, reason}` (reasons: `class_never_on_mcp`, `request_only`, `human_only`); the registry manifest JSON (hash its bytes with SHA-256; read `commands[].id`); `surfaces.toml` listing the surfaces the release ships.
>
> `fid-outreach ref-render --records <dir> --manifest <file> --surfaces <file> --release <tag> --out <dir>`: refuse (exit 1, write nothing) if any record's `manifest_digest` differs from the manifest digest or a manifest command has no record. Write three HTML fragments per record at `<out>/<release>/ref/<cli|mcp|python>/<command_id>@<major>.html` with fixed labels only; help text verbatim and escaped. MCP page: tool name and input schema, or "not available on MCP" plus the reason in words. Python page: the Python name if `python` is in `surfaces.toml`, else a statement that Python bindings are not part of this release and no signature.
>
> `fid-outreach ref-check ... --ref <dir> [--json]`: render in memory, compare bytes, report `record_missing`, `page_missing`, `page_without_command`, `page_drift`; exit 0/1/2; RFC 8785 JSON report sorted by path.
>
> Acceptance tests under `fixtures/outr/docs/ref/` as listed in the spec (paths, MCP reason wording per class, Python page with and without the surface, clean check, drift, extra page, missing record, stale digest, byte-identical reruns).
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing (records over 1 MB refused); no network; BTreeMap only; never add prose of your own to pages; allowlisted dependency licences.

## Open questions

- D-026 and launch-1 exclude AUTO.PY, so this leaf does not depend on it: the Python page is rendered from the registry-derived Python name in the record, and states that bindings are absent when the release does not ship them. This keeps REQ-OUTR.DOCS-001 (one Python page per command) true without Python bindings; a reviewer may still prefer to qualify REQ-OUTR.DOCS-001 with "for each surface the release ships".
- CONSTITUTION §2.1 (D-034): the former code dependencies on CORE.REG, AUTO.CLI, AUTO.MCP and AUTO.PY are replaced by `uses_artifacts` on CORE.REG.MAN (manifest digest and IDs) and AUTO.SURF.DOCS (`docs-ref-v1` records), as AUTO.SURF.DOCS's own Open questions propose.
- AUTO.SURF.DOCS is still open; if its record schema changes, only the input contract above changes.

## Children

_Leaf: none._
