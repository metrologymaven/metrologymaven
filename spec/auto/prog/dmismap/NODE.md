---
id: AUTO.PROG.DMISMAP
title: DMIS mapping contract
level: 3
parent: AUTO.PROG
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-015]
covers: [PCD.PRG.10]
depends_on: [AUTO.PROG.FMT]
uses_artifacts: [IO.DMIS.PARSE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AUTO.PROG.DMISMAP: DMIS mapping contract

## Purpose

A machine-readable table stating, for each DMIS 5.3 statement class that `IO.DMIS.PARSE` recognises, the Command Program construct it maps to (a control-flow construct, an operator step, a variable operation, or a command family owned by CMM) or the reason it is unsupported. `IO.DMIS.PROG` reports against it, `CMM.GEN.DMIS` converts with it in both directions, and conversion reports cite its row IDs and table version. This node owns the table format, the CI check and the review rule; the row meanings are data reviewed by DMIS experts.

## Scope

**In:**
- Table file `data/dmis-map/dmis-map.toml` with format `fid.dmis-map` version 1 and its JSON Schema
- Row fields: statement class key, mapping kind, target (FMT step kind or command-family pattern), status, reason code, reviewers
- Draft rows for the program-structure classes (control flow, variables, declarations, operator text, subroutines and macros) and placeholder rows pointing to CMM command families for measurement and motion classes
- CI check against the statement-class list that `IO.DMIS.PARSE` publishes, and against FMT step kinds
- Generated support summary `docs/dmis/support.md`
- Review rule: a row counts as mapped only with two reviewer sign-offs

**Out:**
- DMIS parsing and writing (`IO.DMIS`)
- Conversion logic and DMIS export of programs (`CMM.GEN.DMIS`)
- Motion and probing semantics and the CMM command families themselves (`CMM`)

## Requirements

- **REQ-AUTO.PROG.DMISMAP-001** The table shall have exactly one row per statement class in the class list published by `IO.DMIS.PARSE`, and the CI check shall fail with `dmis_class_unmapped` or `dmis_class_unknown` naming each missing or extra class.
- **REQ-AUTO.PROG.DMISMAP-002** Each row shall have mapping kind `program_construct`, `command_family` or `unsupported`; a `program_construct` row shall name an FMT step kind, and a `command_family` row shall name a command-ID pattern of the form `<domain>.<verb>_*`.
- **REQ-AUTO.PROG.DMISMAP-003** Each `unsupported` row shall give a reason code from the closed set `out_of_scope`, `not_yet_implemented`, `vendor_specific`, `pending_review`.
- **REQ-AUTO.PROG.DMISMAP-004** A row whose `reviewers` list has fewer than two distinct names shall be published in the generated table as `unsupported` with reason `pending_review`, whatever its drafted mapping.
- **REQ-AUTO.PROG.DMISMAP-005** The table shall carry an integer version that increases with every content change, and the generated file shall record the version and the SHA-256 of the canonical table.
- **REQ-AUTO.PROG.DMISMAP-006** The table shall contain no text copied from the DMIS standard: notes are project-authored, and a CI check shall fail if any note exceeds 200 characters [unverified as a sufficient guard] or contains a statement-syntax excerpt longer than the class keyword.

## Acceptance criteria

- **AC-1** Given fixture class list `fixtures/dmis/classes-min.json` (12 classes) and fixture table `fixtures/dmis/map-min.toml`, when the CI check runs, then it passes and the generated summary lists 12 rows.
- **AC-2** (error path) Given a class list with one more class than the table, then the check fails with `dmis_class_unmapped` naming it; given a table row for a class not in the list, `dmis_class_unknown`.
- **AC-3** (error path) Given a `program_construct` row targeting `goto` (not an FMT step kind), then the check fails with `bad_target`.
- **AC-4** Given a row with one reviewer, then the generated table publishes it as `unsupported` / `pending_review` and the summary counts it as pending.
- **AC-5** Given the same table on Windows and Linux, then the generated file and its digest are byte-identical.

## Design

- **Registry command(s):** none; data file plus build-time check in `cargo xtask gen-dmis-map`.
- **Data structures:** `DmisMap { format, version: u32, rows: Vec<Row> }`; `Row { class: String, kind: ProgramConstruct | CommandFamily | Unsupported, target: Option<String>, reason: Option<Reason>, note: Option<String>, reviewers: Vec<String> }`; generated `dmis-map.json` (RFC 8785, sorted by class).
- **Algorithm:** parse table (bounded, 1 MiB); load class list artifact; set-compare; validate targets against FMT step kinds and the command-ID pattern grammar; apply the review rule; emit canonical JSON and the markdown summary.
- **Errors:** `dmis_class_unmapped`, `dmis_class_unknown`, `bad_target`, `bad_reason`, `note_too_long`.
- **Determinism:** D0.
- **Performance budget:** negligible.
- **Crate:** `fid-program` (module `dmismap`), run from `xtask`.

## Test fixtures

`fixtures/dmis/classes-min.json` and `fixtures/dmis/map-min.toml` (project-authored, CC-BY-4.0; class names are keywords only, no standard text).

## Build prompt

> In crate `fid-program`, add module `dmismap` and an `xtask gen-dmis-map` step. Format `fid.dmis-map` v1 (TOML): integer `version`, rows `{ class, kind: program_construct|command_family|unsupported, target, reason, note, reviewers }`; publish the JSON Schema. Check: exactly one row per class in the statement-class list published by IO.DMIS.PARSE (`dmis_class_unmapped`, `dmis_class_unknown`); `program_construct` targets must be FMT step kinds, `command_family` targets must match `<domain>.<verb>_*` (`bad_target`); unsupported rows need a reason from out_of_scope, not_yet_implemented, vendor_specific, pending_review; rows with fewer than two distinct reviewers are published as unsupported/pending_review; notes ≤ 200 characters. Emit `dmis-map.json` (RFC 8785, sorted, with version and SHA-256) and `docs/dmis/support.md`. Acceptance: AC-1..AC-5. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; never include DMIS standard text (AGENTS.md rule 5); bounded parsing.

## Open questions

- Row content needs DMIS domain experts. This node applies the spirit of D-009 (two-expert sign-off per row) without a new decision; whether the D-009 reviewer pool also covers DMIS is for the maintainers.
- `IO.DMIS.PARSE` must name its statement-class list file and schema version in its Interfaces (artifact dependency per D-034); it does not yet.
- Phase P2 (later than the parent's P1b) matches DMIS program import in `IO.DMIS.PROG`.

## Children

_Leaf: none._
