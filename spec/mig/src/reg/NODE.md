---
id: MIG.SRC.REG
title: Source register, review records, listing and sources page
level: 3
parent: MIG.SRC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-MIG.SRC-001, REQ-MIG.SRC-002, REQ-MIG.SRC-005, REQ-MIG.SRC-006]
covers: []
depends_on: [IO.REG.DETECT, CORE.JRN.CANON]
uses_artifacts: [ECO.PARITY.GEN]
side_effect: R0
determinism: D0
cli: [fid migrate list-sources]
mcp: [migrate_list_sources]
journal: [migrate.list_sources@1]
updated: 2026-11-17
---

# MIG.SRC.REG: Source register, review records, listing and sources page

## Purpose

The source register as data (`mig-sources@1`): one entry per incumbent artefact kind that MIG may be asked to read, with its access route and admission status, and for every non-admitted artefact the open format the user should export to instead. `ADMITTED` needs a linked review record. The register is listed by `migrate.list_sources@1`, and the same output generates the public sources page, so the page can never say more than the register.

## Scope

**In:**
- Register schema: entry ID (`src.<slug>`), incumbent product name (identification only), artefact kind (`results`, `points`, `project`, `form`, `program`), format ID (from the format registry) or `native:<slug>` for unregistered native formats, version range, access route (`PUBLIC_SPEC`, `OPEN_EXPORT`, `NONE`), status, export route (open format ID plus a note in our own words), review record reference
- Review record schema (`mig-review@1`): entry ID, basis relied on, reviewer, date, clean-room statement, EULA note, counsel reference when required
- Register policy field bound to D-033: while policy is `D-033-A`, no entry with access route `PUBLIC_SPEC` for a native format may be `ADMITTED`
- `cargo xtask mig sources-check`: schema, uniqueness, review-record and policy checks, and a vocabulary check
- `migrate.list_sources@1` (R0) and the Markdown page renderer (`--format md`) with the generated header

**Out:**
- The runtime gate and the build check against MIG commands (owned by `MIG.SRC.GATE`)
- Copy detection against incumbent text (owned by `OUTR.BRAND`, D-013)

## Requirements

- **REQ-MIG.SRC.REG-001** Each register entry shall have a unique ID, an incumbent product name, an artefact kind, a format ID, a version range, an access route, a status from `ADMITTED`, `OPEN_EXPORT_ONLY`, `NOT_ADMITTED`, `PENDING_REVIEW`, and, unless `ADMITTED` or `OPEN_EXPORT_ONLY`, an export route naming an open format ID or `none`.
- **REQ-MIG.SRC.REG-002** If two entries have the same incumbent product, artefact kind, format ID and overlapping version ranges, then the sources check shall fail with `SRC-DUP`.
- **REQ-MIG.SRC.REG-003** If an entry is `ADMITTED` or `OPEN_EXPORT_ONLY` without a review record that names the basis relied on, the reviewer, the date and the clean-room statement, then the sources check shall fail with `SRC-REVIEW`.
- **REQ-MIG.SRC.REG-004** While the register policy is `D-033-A`, if an entry for a native (non-open) format is `ADMITTED`, then the sources check shall fail with `SRC-POLICY`.
- **REQ-MIG.SRC.REG-005** Every review record shall carry an attestation that the entry's notes contain no text copied from incumbent documentation, and the sources check shall fail with `SRC-ATTEST` when it is missing.
- **REQ-MIG.SRC.REG-006** When `migrate.list_sources@1` runs, the system shall return every entry sorted by ID with status, access route, export route and register version, optionally filtered by artefact kind or status.
- **REQ-MIG.SRC.REG-007** The sources page shall be rendered only from `migrate.list_sources@1` output with the generated header, and identical register data shall render a byte-identical page.

## Acceptance criteria

- **AC-1** Given `fixtures/mig/src/reg/register-ok.json` and its review records, when `cargo xtask mig sources-check` runs, then it exits 0.
- **AC-2** (error path) Given `admitted-noreview.json`, then it exits 1 with `SRC-REVIEW <id>`.
- **AC-3** (error path) Given `native-admitted.json` (a native program format `ADMITTED` under policy `D-033-A`), then it exits 1 with `SRC-POLICY <id>`.
- **AC-4** (error path) Given `dup.json`, then it exits 1 with `SRC-DUP`.
- **AC-5** Given `register-ok.json`, when `fid migrate list-sources --status NOT_ADMITTED --json` runs, then exactly the not-admitted entries are returned with their export routes.
- **AC-6** Given `register-ok.json`, when `fid migrate list-sources --format md` runs twice, then both outputs equal `expected/sources.md` byte for byte.

## Design

- **Registry command(s):** `migrate.list_sources@1`, R0; params `kind?: ArtefactKind`, `status?: Status`, `format?: json | md`; result `{ register_version, entries: Vec<SourceEntry> }` or the rendered page.
- **Data structures:** `SourceRegister { schema: "mig-sources@1", version, policy: "D-033-A" | "D-033-B", entries }`, `SourceEntry { id, product, kind, format_id, versions, access_route, status, export_route?, review? }`, `ReviewRecord { schema: "mig-review@1", entry, basis, reviewer, date, clean_room_statement, eula_note, counsel_ref?, no_copied_text_attestation }`. The register ships embedded in the build (`include_bytes!`) so the gate needs no file I/O.
- **Algorithm:** bounded parse → schema → duplicate check by (product, kind, format, version overlap) → review and attestation checks → policy rule → render.
- **Errors:** `SRC-SCHEMA`, `SRC-DUP`, `SRC-REVIEW`, `SRC-POLICY`, `SRC-ATTEST`.
- **Determinism:** D0.
- **Performance budget:** under 20 ms [unverified until measured].
- **Crate:** `fid-mig` (module `src::reg`).

## Test fixtures

`fixtures/mig/src/reg/`: synthetic registers naming fictional products (`DemoCMM`, `DemoScan`) so fixtures contain no real incumbent claims; CC0-1.0. The production register is reviewed by a human before each release.

## Build prompt

> In crate `fid-mig`, add module `src::reg` with the `mig-sources@1` register and `mig-review@1` review-record types, the embedded register data, `cargo xtask mig sources-check` and the command `migrate.list_sources@1` (R0; CLI `fid migrate list-sources`, MCP `migrate_list_sources`). Implement spec node MIG.SRC.REG: unique entries; `SRC-DUP` for same product/kind/format with overlapping versions; `SRC-REVIEW` when `ADMITTED`/`OPEN_EXPORT_ONLY` lacks a review record with basis, reviewer, date and clean-room statement; `SRC-ATTEST` when the no-copied-text attestation is missing; `SRC-POLICY` when a native format is `ADMITTED` while the policy is `D-033-A`. `list_sources` returns entries sorted by ID with filters; `--format md` renders the public sources page with the generated-report header, byte-identical for identical data. Write all notes in original wording; use incumbent product names only to identify products; fixtures use fictional product names. Tests for AC-1..AC-6. No `unwrap`/`expect`/`panic`/`unsafe`; BTreeMap only.

## Open questions

- The production register's initial content (which incumbent products and export routes to list) needs a human reviewer and, per D-033's condition, counsel review before release [unverified export routes].

## Children

_Leaf: none._
