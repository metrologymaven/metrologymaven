---
id: AUTO.RUN.PLAY.SELECT
title: Released program and variant list for operators
level: 4
parent: AUTO.RUN.PLAY
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.RUN.PLAY-001, REQ-AUTO.RUN.PLAY-003]
covers: []
depends_on: [AUTO.REC.LIFE, AUTO.PROG.FAMILY, CORE.PRJ.META]
side_effect: R0
determinism: D0
cli: [fid program list-operator-programs]
mcp: [program_list_operator_programs]
journal: [program.list_operator_programs@1]
updated: 2026-11-17
---

# AUTO.RUN.PLAY.SELECT: Released program and variant list for operators

## Purpose

`program.list_operator_programs@1` answers "what may an operator run here?": every program in lifecycle state `released`, with its released version, digest, declared variants and declared operator inputs, optionally filtered to programs tagged for a station. The kiosk chooser (`VIZ.KIOSK.PROGPICK`) shows this list, and `AUTO.RUN.PLAY.SESSION` re-checks the choice when the run starts, so a program that leaves `released` between listing and starting is still refused.

## Scope

**In:**
- `program.list_operator_programs@1` (R0)
- Filtering to released programs; optional station filter by tag `station:<id>` (CORE.PRJ.META tags)
- Per entry: program ID, title, version, digest, variants (names only), and the parameters an operator must supply (no default)

**Out** (and which node owns it instead):
- Lifecycle states (`AUTO.REC.LIFE`); variant declarations (`AUTO.PROG.FAMILY`); tags (`CORE.PRJ.META`)
- Re-checking at start (`AUTO.RUN.PLAY.SESSION`); chooser UI and serial entry (`VIZ.KIOSK.PROGPICK`)

## Requirements

- **REQ-AUTO.RUN.PLAY.SELECT-001** The command shall list only programs whose lifecycle state is `released`, and shall never list draft, registered or deprecated programs.
- **REQ-AUTO.RUN.PLAY.SELECT-002** When a `station` parameter is given, the command shall list only released programs tagged `station:<station>`.
- **REQ-AUTO.RUN.PLAY.SELECT-003** Each entry shall hold program ID, title, released version, program digest, declared variant names in declaration order, and the names, types and units of parameters that have no default.
- **REQ-AUTO.RUN.PLAY.SELECT-004** The list shall be ordered by title, then program ID, in byte order.
- **REQ-AUTO.RUN.PLAY.SELECT-005** Program titles and variant names shall be returned typed as untrusted text (C6).

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/projects/kiosk-programs/` (programs: `bracket` released, `cover` draft, `flange` registered, `hinge` deprecated, `plate` released and tagged `station:S1`), when `fid program list-operator-programs --json` runs, then exactly `bracket` and `plate` are listed, in that order.
- **AC-2** Given the same project, when run with `--station S1`, then only `plate` is listed.
- **AC-3** Given `bracket`, then its entry lists variants `B-100`, `B-120` in declaration order and parameter `scan` as required.
- **AC-4** Given the same project twice, then the outputs are byte-identical.

## Design

- **Registry command(s):** `program.list_operator_programs@1`, R0 (MCP exposed); params `station: Option<String>`; result `Vec<OperatorProgramEntry>`. Not journaled to the Record Journal (D-031: Activity Log only while recording or inside a program).
- **Data structures:** `OperatorProgramEntry { program_id, title: Untrusted<String>, version, digest, variants: Vec<Untrusted<String>>, required_params: Vec<{ name, ty, unit }> }`.
- **Algorithm:** query lifecycle states (`AUTO.REC.LIFE`), keep `released`; filter by tag; read parameter and variant declarations (`AUTO.PROG.FAMILY`); sort.
- **Errors:** `project_not_open`.
- **Determinism:** D0.
- **Performance budget:** 1,000 programs listed in < 50 ms [unverified until measured].
- **Crate:** `fid-run` (module `play::select`)

## Test fixtures

New `fixtures/projects/kiosk-programs/` built by a seeded generator (five programs in the states above, one station tag); project-authored.

## Build prompt

> In crate `fid-run`, add module `play::select` with registry command `program.list_operator_programs@1` (R0; CLI `fid program list-operator-programs`, MCP `program_list_operator_programs`). Return every program whose lifecycle state is `released` (never draft, registered or deprecated), optionally only those tagged `station:<id>` when `station` is given. Each entry: program ID, title (untrusted text), released version, digest, variant names in declaration order (untrusted text), and the name, type and unit of every parameter without a default. Sort by title then program ID in byte order. Acceptance: AC-1..AC-4 with `fixtures/projects/kiosk-programs/`. Constraints: CONSTITUTION §8 (no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`); ordered collections only; C6 untrusted text.

## Open questions

- Station assignment by the tag `station:<id>` is an assumption; `VIZ.KIOSK.PROGPICK` asks whether it belongs in AUTO.PROG metadata. A tag needs no new schema; if AUTO.PROG later adds a field, only the filter changes.

## Children

_Leaf: none._
