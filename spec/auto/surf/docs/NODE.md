---
id: AUTO.SURF.DOCS
title: Generated command reference records
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.SURF-001, REQ-AUTO.SURF-002]
covers: []
depends_on: [AUTO.SURF.GEN, AUTO.SURF.PROJ]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.SURF.DOCS: Generated command reference records

## Purpose

The docs emitter (`SurfaceEmitter` for surface `docs`). For every command major in the projection it writes one structured, canonical **reference record**: summary and help text, parameters with types and units, result type, side-effect class, determinism class, and for each surface (CLI, MCP, Python) whether and how the command is reachable. It also writes per-domain index records and a side-effect class legend. The docs portal (`OUTR.DOCS.REF`) renders these records into its per-release CLI, MCP and Python pages. This node settles the question `OUTR.DOCS.REF` left to AUTO by emitting **structured data, not finished pages**, so the page format and link scheme stay with the portal and this leaf is not blocked on them.

Help text has one source: the projection's `help: DocText` field (`AUTO.SURF.PROJ`). `AUTO.CLI.TREE` renders `--help` from that same field, and this emitter copies it verbatim into the record, so CLI help and the docs cannot disagree for one manifest digest.

## Scope

**In:**
- Docs emitter implementing `SurfaceEmitter` (`AUTO.SURF.GEN`), surface name `docs`
- Reference record schema `docs-ref-v1` (one JSON file per command major)
- Per-domain index records and the class legend record
- Per-surface reachability statement derived from the projection (exposed, named tool, request only, not on MCP, with reason)
- Example blocks copied only from registry-declared examples carried in the projection's `DocText`

**Out** (and which node owns it instead):
- Rendering records into HTML or Markdown pages, URLs, navigation, editions (`OUTR.DOCS.REF`, `OUTR.DOCS.FRAME`)
- The page drift check in the docs build (`OUTR.DOCS.REF`); the CI surface-completeness and staleness check (`AUTO.SURF.CHECK`)
- Rendering CLI help (`AUTO.CLI.TREE`)
- Tutorials, how-tos and Skills (`AUTO.MCP.SKILLS`, `OUTR.CONTENT`)
- File headers, `surface-index.json`, byte determinism of written files (`AUTO.SURF.GEN`)

## Requirements

- **REQ-AUTO.SURF.DOCS-001** The emitter shall return exactly one reference record file `ref/<command_id>@<major>.json` per command major in the projection, and shall declare that command major in the file's `covers` so that `AUTO.SURF.GEN` lists it in the docs `surface-index.json`.
- **REQ-AUTO.SURF.DOCS-002** Each reference record shall state the command's side-effect class, determinism class, `human_only` flag and, for each of the surfaces `cli`, `mcp` and `python`, either the derived name or `available: false`, with a reason code from the closed set `class_never_on_mcp`, `request_only`, `human_only` whenever the command is not a plain MCP tool.
- **REQ-AUTO.SURF.DOCS-003** The record's `help` field shall be a verbatim copy of the projection's `help: DocText` value, with no rewording, truncation or added text.
- **REQ-AUTO.SURF.DOCS-004** The emitter shall add no text to a record other than values from the projection and the fixed field names and reason codes of the `docs-ref-v1` schema, so a record contains no standards text that the registry declaration does not already contain.
- **REQ-AUTO.SURF.DOCS-005** Each reference record shall name the manifest SHA-256 digest and the command definition's own digest as given by the projection.
- **REQ-AUTO.SURF.DOCS-006** The emitter shall write one index record `index/<domain>.json` per domain, listing that domain's command majors in byte order, and one legend record `classes.json` listing every side-effect class with its MCP projection.
- **REQ-AUTO.SURF.DOCS-007** Records shall be serialised as RFC 8785 canonical JSON, so that two emissions from the same projection are byte-identical.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json` (one command per side-effect class), when `cargo xtask gen-surfaces --manifest fixtures/manifest/min-classes.json --out target/gen` runs, then `target/gen/docs/ref/` holds exactly one record per command major, `target/gen/docs/surface-index.json` lists each of them, and each record equals its golden under `fixtures/docs/ref/`.
- **AC-2** Given the same run, then the M1, S1 and A0 records have `mcp: {"available": false, "reason": "class_never_on_mcp"}`, the W3, X1 and D1 records have `mcp.reason = "request_only"` with the tool name present, and the R0 record has `mcp.available = true` with the derived tool name and no reason.
- **AC-3** Given fixture `fixtures/manifest/human-only.json` with a W2 command marked `human_only: true`, then its record has `human_only: true` and `mcp.reason = "human_only"`.
- **AC-4** Given the same manifest, when the record's `help` value is compared with the body that `fid align run-bestfit --help` prints from a CLI built against the same manifest, then the help body bytes are equal.
- **AC-5** Given two emissions into two directories, then the `docs/` trees are byte-identical, and every record names the manifest digest of `min-classes.json`.
- **AC-6** (error path) Given a projection whose `DocText` for one command is empty, then emission fails with `missing_help` naming that command, and `AUTO.SURF.GEN` writes nothing.

## Design

- **Registry command(s):** none; runs inside `cargo xtask gen-surfaces` (`AUTO.SURF.GEN`).
- **Data structures:** `RefRecord { schema: "docs-ref-v1", manifest_digest, command_digest, command_id, major, summary, help: DocText, params: Vec<ParamDoc { name, ty, quantity_kind, unit, default }>, result: TypeRef, side_effect, determinism, human_only, long_running, dry_run, surfaces: { cli: SurfaceDoc, mcp: SurfaceDoc, python: SurfaceDoc } }`; `SurfaceDoc { available: bool, name: Option<String>, projection: Option<McpProjection>, reason: Option<ReasonCode> }`; `DomainIndex { domain, commands: Vec<CommandRef> }`; `ClassLegend { classes: Vec<{ code, mcp_projection }> }`.
- **Algorithm:** for each `SurfaceProjection` in projection order, build the `RefRecord`. MCP reachability: projection `none` gives `available: false, reason: class_never_on_mcp`; `request_only` gives the name with reason `request_only`; `human_only: true` gives reason `human_only` and overrides `named` or `exposed`. Then group by domain for the indexes, emit the legend from the fixed class table of CONSTITUTION §3, and serialise with the shared RFC 8785 serialiser (`CORE.JRN.CANON`, reached through `AUTO.SURF.PROJ`).
- **Errors:** `missing_help`; `emit_failed` (wrapped by GEN).
- **Determinism:** D0: a pure function of the projection; ordered maps only; no clock, locale or host data.
- **Performance budget:** 5,000 command majors emitted in < 1 s on reference hardware [unverified until measured].
- **Crate:** `fid-surface-gen` (module `docs`).
- **Publishes (artifact):** `docs/ref/*.json`, `docs/index/*.json`, `docs/classes.json`, and schema `schema/docs-ref-v1.json` generated from the Rust types. `OUTR.DOCS.REF` consumes them as an artifact.

## Test fixtures

`fixtures/manifest/min-classes.json` (shared with `AUTO.SURF.PROJ`), new `fixtures/manifest/human-only.json`, goldens under `fixtures/docs/ref/` and `fixtures/docs/index/`; all synthetic and project-authored (CC-BY-4.0).

## Build prompt

> In crate `fid-surface-gen`, add module `docs` with a `DocsEmitter` implementing the existing `SurfaceEmitter` trait (surface name `docs`). For each `SurfaceProjection` (command major), emit `ref/<command_id>@<major>.json`, a `docs-ref-v1` reference record holding: schema name, manifest SHA-256, command definition digest, command ID and major, summary, `help` copied verbatim from the projection's `DocText`, parameters (name, type, quantity kind, unit, default), result type, side-effect class, determinism class, `human_only`, `long_running`, `dry_run`, and a `surfaces` object with `cli`, `mcp` and `python` entries `{available, name, projection, reason}`. MCP reason codes: projection `none` → `class_never_on_mcp` (unavailable); `request_only` → `request_only` (name present); `human_only: true` → `human_only` (overrides named or exposed). Also emit `index/<domain>.json` (command majors in byte order) and `classes.json` (each side-effect class with its MCP projection: R0/R1/I1 exposed, W1/W2 named, W3/X1/D1 request_only, M1/S1/A0 none). Declare each record's command in `GeneratedFile.covers`. Serialise everything with the project's RFC 8785 canonical JSON serialiser and generate `schema/docs-ref-v1.json` from the Rust types. Add no prose of your own: only projection values and fixed schema keys and codes. Fail with `missing_help` if a command's `DocText` is empty. Acceptance: golden records for `fixtures/manifest/min-classes.json`; reason codes for M1/S1/A0, W3/X1/D1 and a `human_only` W2 command; help bytes equal to the `fid <cmd> --help` body for the same manifest; byte-identical double emission; the `missing_help` error writes nothing. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); `BTreeMap` only, never `HashMap`; never store or emit standards text.

## Open questions

- The projection (`AUTO.SURF.PROJ`, leaf) does not yet carry `human_only` (D-031), registry-declared examples or the command definition digest. This leaf assumes `SurfaceProjection` gains `human_only: bool`, `command_digest: Sha256` and `DocText.examples`. Recorded for whoever next revises `AUTO.SURF.PROJ` (not edited from this node). Until then the emitter treats `human_only` as false and emits no examples, which keeps it buildable.
- `OUTR.DOCS.REF` should declare `uses_artifacts: [AUTO.SURF.DOCS]` and render `docs-ref-v1` records deterministically; its CLI, MCP and Python pages are three renderings of one record. Cross-domain note for OUTR; not edited from here.
- The earlier note "page format depends on `OUTR.DOCS`" is resolved by emitting structured records: `OUTR.DOCS.REF` left that choice to AUTO.

## Children

_Leaf: none._
