---
id: UX.PATTERN.SCHEMA
title: Pattern-spec format and completeness check
level: 3
parent: UX.PATTERN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.PATTERN-001]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.COMP.SCHEMA]
uses_artifacts: [CORE.REG.MAN, FLOW.PERSONA.MAPS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.PATTERN.SCHEMA: Pattern-spec format and completeness check

## Purpose

The format every interaction pattern is written in and the check that each one defines its trigger, steps, states, the command patterns it invokes, its keyboard path, and its cancel and failure paths, and links to the components it uses and the journey steps it serves.

## Scope

**In:** `pattern-spec@1`; `patterns/TEMPLATE.md`; `fid-design pattern-check`.
**Out:** the patterns (sibling PATTERN nodes); component specs (`UX.COMP`); journey maps (FLOW.PERSONA).

## Requirements

- **REQ-UX.PATTERN.SCHEMA-001** The front block shall hold `id` (`pat.<name>`), `trigger`, `steps` (numbered, each with actor, components used and expected input), `states`, `commands` (registry command ID patterns such as `view.select_*@1`), `keyboard` (one complete keyboard path), `cancel` and `failure` paths, and `journeys` (journey map IDs and step numbers it serves).
- **REQ-UX.PATTERN.SCHEMA-002** If a named component is not a `comp.*` spec in the package, then `pattern-check` shall fail and name it.
- **REQ-UX.PATTERN.SCHEMA-003** If a `commands` pattern matches no command in the given registry manifest, then `pattern-check` shall report it as a warning `PAT-COMMAND-UNMATCHED`.
- **REQ-UX.PATTERN.SCHEMA-004** If `cancel`, `failure` or `keyboard` is empty, then `pattern-check` shall fail.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/pattern/schema/ok.md`, the package fixture and a manifest fixture, when `fid-design pattern-check --root ok --package pkg --manifest manifest.json --json` runs, then it exits 0.
- **AC-2** (error path) Given `no-cancel.md`, when the check runs, then it exits 1 with `PAT-CANCEL-MISSING`.
- **AC-3** (error path) Given `bad-comp.md` (uses `comp.nope`), when the check runs, then it exits 1 with `PAT-COMPONENT-UNKNOWN comp.nope`.
- **AC-4** Given `stale-cmd.md` (`commands = ["foo.bar_*@1"]`), when the check runs, then it exits 0 with warning `PAT-COMMAND-UNMATCHED`.

## Design

- **Command:** `fid-design pattern-check --root <patterns dir> --package <dir> [--manifest <file>] [--journeys <dir>] [--json]`. Tooling, nearest class R0. With `--journeys`, unknown journey map IDs are reported as `PAT-JOURNEY-UNKNOWN`.
- **Errors:** `PAT-SCHEMA`, `PAT-CANCEL-MISSING`, `PAT-FAILURE-MISSING`, `PAT-KEYBOARD-MISSING`, `PAT-COMPONENT-UNKNOWN`, `PAT-JOURNEY-UNKNOWN`; warning `PAT-COMMAND-UNMATCHED`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `pattern`.
- **Pinned choices (build):**
  - **Format:** the front block is split as `component-spec@1`'s; closed key set `format`, `id`, `title` (optional), `trigger`, `states`, `commands`, `cancel`, `failure`, `[[steps]]` `{n, actor, components, input}`, `[[keyboard]]` `{step, keys, note?}`, `[[journeys]]` `{map, steps}`. Steps are numbered 1, 2, 3, .. in order; `actor` is one of the FLOW.PERSONA.MAPS journey actors (`human`, `operator`, `admin`, `agent`, `system`). `cancel` and `failure` are arrays of text lines. `states` and `journeys` need at least one entry; `commands = []` is allowed (a pattern that runs no command).
  - **Complete keyboard path:** every step whose actor is a person (`human`, `operator`, `admin`) needs a `[[keyboard]]` entry, else `PAT-KEYBOARD-MISSING`.
  - **Components:** a component is known when `<package>/components/*.md` holds a `component-spec@1` front block with that `comp.<name>` id; whether that spec passes `comp-check` is `comp-check`'s job.
  - **Command patterns:** `<name glob>@<major>`; the glob uses `[a-z0-9_.]` plus `*` (any run of name characters), starts with a literal letter or digit, at most 8 `*`, never `**`; the major is exact. The manifest is CORE.REG.MAN's (only `id` and `major` read, the person view; D-035's agent view only hides commands from agents); a malformed manifest exits 2.
  - **Fail closed:** without `--manifest` or `--journeys` the report lists `commands` or `journeys` under `unchecked` and `ok` is false, while the exit code still follows the errors only. A `--journeys` file that is not a well-formed `journey-map@1` is `PAT-SCHEMA` naming the file. `PAT-JOURNEY-UNKNOWN` names `<map>` or `<map>/<step>`.
  - **Fixture layout:** the AC files live at `fixtures/ux/pattern/schema/ok/ok.md` (so `--root ok` is a directory), `no-cancel.md`, `bad-comp.md`, `stale-cmd.md`, `manifest.json`, with the package fixture `pkg/`; `make_fixtures.py` holds the hand labels and an independent stdlib reference checker.

## Test fixtures

`fixtures/ux/pattern/schema/`: `ok.md`, `no-cancel.md`, `bad-comp.md`, `stale-cmd.md`, `manifest.json`; synthetic, CC-BY-4.0.

## Build prompt

> Define `pattern-spec@1` (Markdown with a fenced TOML front block: id `pat.<name>`, trigger, steps [{n, actor, components, input}], states, commands (command ID patterns with `*`), keyboard, cancel, failure, journeys [{map, steps}]) and ship `patterns/TEMPLATE.md`. In the Rust tooling crate `fid-design`, add `fid-design pattern-check --root <dir> --package <dir> [--manifest <file>] [--journeys <dir>] [--json]` with the errors and warning listed in spec node UX.PATTERN.SCHEMA. Exit 0 (warnings allowed), 1 errors, 2 unreadable. Tests AC-1..AC-4.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; no network.

## Open questions

- None.

## Children

_Leaf: none._
