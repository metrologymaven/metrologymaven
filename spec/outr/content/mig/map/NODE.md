---
id: OUTR.CONTENT.MIG.MAP
title: Migration mapping table generator and drift check
level: 4
parent: OUTR.CONTENT.MIG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.MIG-001, REQ-OUTR.CONTENT.MIG-002, REQ-OUTR.CONTENT.MIG-003]
covers: []
depends_on: []
uses_artifacts: [ECO.PARITY.TRACE, ECO.COMM.SCOPE]
side_effect: [W1, R0]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.MIG.MAP: Migration mapping table generator and drift check

## Purpose

Generates the mapping table that every migration guide embeds: one row per in-scope row of the matching seed inventory, naming our command(s) when the row is live in the release's parity trace, or marking it as a gap with the spec node that will cover it. A check mode regenerates every table and fails the content build when an embedded table differs, so no guide can claim a mapping the parity data does not support. It is outreach tooling, not a product registry command.

## Scope

**In:** the `mig-map gen` and `mig-map check` subcommands; the table format and the begin/end markers inside a guide; the rule that turns a command ID into its CLI form for display.
**Out:** the parity trace itself (`ECO.PARITY.TRACE`); the coverage report naming the deepest covering node (`ECO.COMM.SCOPE`, `generated/COVERAGE.md`); the guide prose (`OUTR.CONTENT.MIG.GOM`, `OUTR.CONTENT.MIG.PW`, `OUTR.CONTENT.MIG.PCD`, `OUTR.CONTENT.MIG.SA`); manifest and claim-lint checks (`OUTR.CONTENT.PUB`).

## Requirements

- **REQ-OUTR.CONTENT.MIG.MAP-001** When `mig-map gen` runs for a guide, it shall write between the guide's `<!-- mig-map:begin -->` and `<!-- mig-map:end -->` markers a table with exactly one row per seed row of the guide's inventory that is not matched by `seeds/out-of-scope.md`, sorted by seed ID.
- **REQ-OUTR.CONTENT.MIG.MAP-002** Each row whose seed is `live` in the given `trace.json` shall list every command ID the trace gives for it, each also shown in the CLI form `fid <domain> <verb-object>` derived by CONSTITUTION §3.
- **REQ-OUTR.CONTENT.MIG.MAP-003** Each row whose seed is not live shall be marked `gap` and name the deepest covering spec node(s) listed for that seed in `generated/COVERAGE.md`.
- **REQ-OUTR.CONTENT.MIG.MAP-004** If a guide's embedded table differs byte-for-byte from a fresh generation from the same inventory, out-of-scope list, trace and coverage report, then `mig-map check` shall exit 1 and name the guide.
- **REQ-OUTR.CONTENT.MIG.MAP-005** The generated table shall carry a header line naming the inventory file, the trace's release and the SHA-256 digests of the trace and coverage inputs.
- **REQ-OUTR.CONTENT.MIG.MAP-006** The tool shall exit 0 when nothing is wrong, 1 on any finding, and 2 when an input cannot be read or parsed.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/mig/basic` (a five-row inventory `seeds/demo.md` with one row in `out-of-scope.md`, a `trace.json` with two live rows and two not-live rows, a `COVERAGE.md`, and a guide with empty markers), when `fid-outreach mig-map gen --guide guide.md --inventory seeds/demo.md --oos seeds/out-of-scope.md --trace trace.json --coverage COVERAGE.md` runs, then the guide's table has four rows, two with commands in ID and CLI form and two marked `gap` with their node IDs.
- **AC-2** Given the AC-1 output, when `mig-map check` runs with the same inputs, then it exits 0.
- **AC-3** (drift) Given fixture `fixtures/outr/mig/drift` (the AC-1 guide with one row hand-edited from `gap` to a command), when `mig-map check` runs, then it exits 1 with finding `table_drift` naming the guide.
- **AC-4** (error path) Given a guide with no begin marker, when `mig-map gen` runs, then it exits 1 with finding `markers_missing` and leaves the file unchanged.
- **AC-5** (error path) Given a `trace.json` that is not valid JSON, when either mode runs, then it exits 2 and writes nothing.
- **AC-6** (determinism) Given fixture `basic`, when `gen` runs twice on fresh copies, then the two guides are byte-identical.

## Design

- **Commands:** `fid-outreach mig-map gen --guide <file> --inventory <seed file> --oos <file> --trace <file> --coverage <file>` (nearest class W1: rewrites only the marked region of a guide in the content repository) and `fid-outreach mig-map check --root <dir> --trace <file> --coverage <file> --seeds <dir> [--json]` (R0). Not registry commands.
- **Guide binding:** the guide's front matter (the OUTR.CONTENT.PUB manifest) gains `inventory: <seed file name>`; `check` walks `--root` for files with `kind: migration` and checks each.
- **Inputs read:** seed tables (rows `| ID | name | notes |`); out-of-scope patterns (first column, `fnmatchcase` semantics); from `trace.json` only `release` and `seeds[] {id, state, commands[]}`; from `COVERAGE.md` only the seed ID and the node IDs in the "Covered by" column.
- **CLI form:** drop `@major`, split `domain.verb_object` at the first dot, replace `_` with `-` in the second part (`align.run_bestfit@1` -> `fid align run-bestfit`).
- **Table columns:** Seed | Workflow (seed row name, already in project words) | Our commands | Status (`live` or `gap`) | Spec node(s). Pipes in names are escaped.
- **Errors:** findings `markers_missing`, `inventory_unknown`, `table_drift`, `seed_not_in_trace` (a non-out-of-scope row absent from the trace); fatal `InputUnreadable`, `InputParse`.
- **Determinism:** D0. Sorted rows, LF line endings, no clock.
- **Performance budget:** all four guides against 1,000 seed rows in under 2 s [unverified until measured].
- **Crate:** `fid-outreach`, module `mig_map`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/mig/` (`basic`, `drift`, `no-marker`, `bad-trace`): a five-row demo inventory written for the test, a hand-made `trace.json` and `COVERAGE.md` in the formats above. CC-BY-4.0.

## Build prompt

> Build the `mig-map` subcommand of the Rust outreach tooling crate `fid-outreach` (not part of the product kernel or registry). Two modes:
>
> `fid-outreach mig-map gen --guide <file> --inventory <seed file> --oos <file> --trace <file> --coverage <file>` rewrites only the text between `<!-- mig-map:begin -->` and `<!-- mig-map:end -->` in a Markdown migration guide. `fid-outreach mig-map check --root <dir> --trace <file> --coverage <file> --seeds <dir> [--json]` finds every guide whose front matter has `kind: migration`, reads its `inventory:` field, regenerates the table in memory and compares bytes.
>
> Table: header line naming the inventory, the trace's `release` and SHA-256 digests of the trace and coverage files; columns Seed | Workflow | Our commands | Status | Spec node(s); one row per inventory row (`| ID | name | notes |`) not matched by any pattern in the out-of-scope file (fnmatchcase), sorted by ID. Live rows (from `trace.json` `seeds[].state == "live"`) list each command ID and its CLI form (`align.run_bestfit@1` -> `fid align run-bestfit`). Other rows say `gap` and list the node IDs from the "Covered by" column of `generated/COVERAGE.md`.
>
> Findings: `markers_missing`, `inventory_unknown`, `table_drift`, `seed_not_in_trace`. Exit 0 clean, 1 with findings, 2 on unreadable or unparsable input (write nothing). Reports are RFC 8785 JSON sorted by file then finding.
>
> Acceptance tests with fixtures under `fixtures/outr/mig/`: `basic` yields four rows (two live with commands, two gap with nodes) and `check` then passes; `drift` fails with `table_drift`; a missing marker fails with `markers_missing` and leaves the file unchanged; a bad trace exits 2; two `gen` runs are byte-identical.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing (refuse inputs over 20 MB), fail closed; no network; BTreeMap only; LF output; dependencies only under allowlisted licences.

## Open questions

- The `trace.json` field names follow the ECO.PARITY.TRACE leaf; if they change, only the input mapping here changes.
- Seed row names are written in project words by seed policy; if a row name ever needs rewording for a guide, the seed file is fixed, not the table.

## Children

_Leaf: none._
