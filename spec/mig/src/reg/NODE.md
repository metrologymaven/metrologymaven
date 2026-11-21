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
updated: 2026-11-21
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

- The registry manifest needs a format catalogue (the IO.REG.DETECT descriptor IDs, for example a top-level `formats` array of `{id}` objects) before any open format can be named here; until then `sources-check` refuses every non-native format. To be settled with CORE.REG.MAN and IO.REG.DETECT.

- `fid migrate list-sources` is not built yet. `fid-cli` has no dispatch path to registry commands: it has no bus client and no registry-built command table (AUTO.SURF), and linking `fid-mig` directly is a surface side door (`lint_crate_graph`, `surface_dep fid-cli -> fid-mig -> fid-registry`). Until AUTO.SURF gives `fid` a bus dispatch, `migrate.list_sources@1` is reachable through the bus and MCP (`migrate_list_sources`) only. AC-5 and AC-6 are tested at the command (`run_list_sources`) in `crates/fid-mig/tests/src_reg.rs`; their CLI form stays open, so this leaf is not marked built.

- The production register's initial content (which incumbent products and export routes to list) needs a human reviewer and, per D-033's condition, counsel review before release [unverified export routes].

## As built

- `fid-mig` module `src::reg`: loader, `check`, `list_sources`, `render_md`, `run_list_sources` and the registry definition of `migrate.list_sources@1` (registered in `fid-registry-build` `SOURCES`). `cargo xtask mig sources-check --register <json> [--vocab-rules <toml>]` in `fid-xtask`. The CLI form `fid migrate list-sources` is not built (see Open questions).
- The review record is inline in its entry (`review`), and its `entry` field must name the entry, so a reference cannot dangle. A review record is self-asserted data: its authority comes from the code-owner-reviewed merge of `data/mig/sources.json` (`.github/CODEOWNERS`), not from its own fields. An export route is `{format, note}`, where `format` is an open format ID or `none`.
- Version ranges are `<lo>..<hi>`. Each end is `*` or a dotted numeric version of 1 to 4 components, both ends are inclusive and missing components count as 0, so `3..5.2` holds 5.2 but not 5.2.1.
- Every free-text field and product name is refused (`SRC-SCHEMA`) if it holds control, line or paragraph separator (U+2028/2029), invisible-operator (U+2060 to U+2064), bidi-mark (U+061C, U+200E/F) or bidi-override characters, Markdown/HTML metacharacters, or a link or address marker (`://`, `www.`, `@`). The page is then rendered without escaping and carries no link from the register.
- "Open format" is checked against the format registry: `sources-check --manifest <registry manifest>` requires every non-`native:` `format_id` and every export-route format to be in the manifest's format catalogue (a top-level `formats` array of `{id}` objects), else `SRC-SCHEMA <entry>.<field>`. **Deviation:** the workspace manifest (CORE.REG.MAN) has no format catalogue yet, so today every non-native format is refused (fail closed); the empty production register passes, and CI checks it against the workspace manifest.
- `counsel_ref` is required (`SRC-REVIEW`) for an `ADMITTED` native format under policy `D-033-B`.
- The vocabulary check runs the shared engine `fid-vocab` with a given rule file (for example the outreach claim rules) over product names, export notes and review texts. Each error is reported as `SRC-VOCAB <id> <rule> <message>`, a code beyond the leaf's error list.
- The embedded production register `data/mig/sources.json` has no entries yet (see Open questions). Without `--vocab-rules`, `sources-check` prints that the vocabulary check was skipped.

## Children

_Leaf: none._
