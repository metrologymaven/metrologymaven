---
id: AUTO.PROG.CHECK
title: Static program checker
level: 3
parent: AUTO.PROG
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG-003]
covers: []
depends_on: [AUTO.PROG.FMT, AUTO.PROG.EXPR.TYPES, AUTO.PROG.FLOW, AUTO.PROG.OPER, AUTO.PROG.FAMILY, CORE.REG]
side_effect: R1
determinism: D0
cli: [fid program check]
mcp: [program_check]
journal: [program.check@1]
updated: 2026-11-17
---

# AUTO.PROG.CHECK: Static program checker

## Purpose

`program.check@1` (R1): runs every static rule over a program against a manifest and returns one ordered diagnostic list plus a summary of what the program may do. It resolves every `invoke` against the manifest (command and major exist, arguments match the parameter schema), runs the expression, flow, operator and family checks, and reports the side-effect class of each step and of the whole program, so a reviewer sees, for example, that a program contains motion steps that will need the approval station before it ever runs. The interpreter refuses to start a program that has check errors.

## Scope

**In:**
- `program.check@1` command, diagnostics model, severities and ordering
- Command resolution and argument-schema checks against the manifest (literal arguments by JSON Schema, expression arguments by inferred type)
- Orchestration of `AUTO.PROG.EXPR.TYPES`, `AUTO.PROG.FLOW`, `AUTO.PROG.OPER` and `AUTO.PROG.FAMILY` checks
- Per-step and whole-program side-effect class summary, including `human_only` steps and M1 steps
- Library entry `check_program` used by `AUTO.PROG.INTERP` and `AUTO.PROG.EDIT`

**Out:**
- The individual rules (`AUTO.PROG.EXPR.TYPES`, `AUTO.PROG.FLOW`, `AUTO.PROG.OPER`, `AUTO.PROG.FAMILY`)
- Parsing and canonical form (`AUTO.PROG.FMT`)
- Refusing a run (`AUTO.PROG.INTERP.ENGINE`)

## Requirements

- **REQ-AUTO.PROG.CHECK-001** If an `invoke` names a command ID or major absent from the manifest, or one whose lifecycle state is removed, then the checker shall report `unknown_command` at that step.
- **REQ-AUTO.PROG.CHECK-002** If an `invoke`'s literal or expression arguments do not match the command's parameter schema (missing required, unknown name, wrong type or quantity kind), then the checker shall report `bad_arguments` naming each offending parameter.
- **REQ-AUTO.PROG.CHECK-003** The checker shall return all diagnostics of all modules ordered by step position in document order, then by module, then by code, each with step ID, optional span, code, severity (`error`, `warning`, `info`) and message.
- **REQ-AUTO.PROG.CHECK-004** The checker shall report for each `invoke` its side-effect class and `human_only` flag, and for the program the highest class over all reachable steps, using the order R0 < R1 < I1 < W1 < W2 < X1 < D1 < W3 < M1 < S1 < A0.
- **REQ-AUTO.PROG.CHECK-005** If a step invokes a deprecated command major, then the checker shall report `deprecated_command` as a warning naming the replacement when the manifest names one.
- **REQ-AUTO.PROG.CHECK-006** `program.check@1` shall be side-effect class R1, change no state, and return byte-identical output for the same program digest and manifest digest.
- **REQ-AUTO.PROG.CHECK-007** The result shall state `passed: true` only when no diagnostic has severity `error`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/check/clean.json` and manifest `fixtures/manifest/prog-demo.json`, when `fid program check --program root:in/clean.json --json` runs, then `passed: true`, no errors, program class `W2` and per-step classes as in `clean.expected.json`.
- **AC-2** (error path) Given `fixtures/program/check/unknown-cmd.json` (invokes `demo.nope@1` and `demo.set_offset@3`), then two `unknown_command` diagnostics at the right step IDs and `passed: false`.
- **AC-3** (error path) Given `fixtures/program/check/bad-args.json` (missing `offset`, unknown `colour`, `offset: 5[deg]`), then one `bad_arguments` naming all three parameters.
- **AC-4** Given `fixtures/program/check/mixed.json` with a type error, an illegal jump, an operator descriptor with a bad unit and an unused parameter binding issue, then the diagnostics come from all four modules in document order.
- **AC-5** Given `fixtures/program/check/motion.json` with one `demo.move_axis@1` (M1) step, then the program class is `M1` and that step is listed with class M1.
- **AC-6** (determinism) Given AC-4 run twice on Windows and Linux, then the JSON outputs are byte-identical, and the Record Journal is unchanged (R1 writes only to the Activity Log when recording).

## Design

- **Registry command(s):** `program.check@1`, R1. Parameters: `program: ProgramRef` (draft or registered program ID, or a read-root handle), `manifest: Option<ManifestRef>` (default: running manifest). Result: `CheckResult { program_digest, manifest_digest, passed, program_class, steps: Vec<StepClass { step_id, command, class, human_only }>, diagnostics: Vec<Diag> }`.
- **Data structures:** `Diag { step_id: Option<StepId>, span: Option<Span>, module: Fmt|Expr|Flow|Oper|Family|Check, code, severity, message }`.
- **Algorithm:** parse (FMT) → type check → flow analysis → operator descriptor validation → family declarations → command resolution and argument schema per `invoke` → class summary over reachable steps (from FLOW) → stable sort.
- **Errors:** command-level `program_not_found`, `bad_handle`; everything else is a diagnostic.
- **Determinism:** D0.
- **Performance budget:** 10 000-step program checked in < 300 ms [unverified until measured].
- **Crate:** `fid-program` (module `check`); command registered from this crate.

## Test fixtures

Programs under `fixtures/program/check/` with expected outputs; `fixtures/manifest/prog-demo.json` with `demo.*` commands of several classes, one deprecated major and one M1 command. Project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-program`, add module `check` and register `program.check@1` (R1; CLI `fid program check`; MCP `program_check`). Input: a program (by program ID or read-root handle) and optionally a manifest (default: running). Steps: parse with FMT; run `expr::types`, `flow`, operator-descriptor validation and family declaration checks; resolve each `invoke` against the manifest (`unknown_command` for absent ID/major or removed; `deprecated_command` warning with replacement; `bad_arguments` naming every offending parameter, using JSON Schema for literals and inferred types for expressions); compute per-step class and `human_only`, and the program's highest class over reachable steps with order R0 < R1 < I1 < W1 < W2 < X1 < D1 < W3 < M1 < S1 < A0. Return diagnostics sorted by document position, module, code; `passed` only with no errors. Expose `check_program()` for the interpreter and editor. Acceptance: AC-1..AC-6. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; R1 writes nothing to the Record Journal (D-031); byte-identical output for the same digests.

## Open questions

- The class order in REQ-004 is this node's reviewer-facing ranking, not a constitution rule; it only orders the summary.
- `deprecated_command` needs the manifest to record a replacement major; `CORE.REG.VER` owns lifecycle data.
- **Partial build (library only), so `built_in` is not set.** `fid-program` module `check` has `check_program()` and `check_in_force()` with every rule of REQ-001 to -007, and `program.check@1` is declared (`fid_program::command_defs`, registered in `fid-registry-build` `SOURCES`, listed in `fid-compose` `UNWIRED`). Open: the handler that reads a program by ID (`AUTO.REC.LIFE` drafts and registrations) or by read-root handle (`CORE.BUS.POL.FS`) and writes only to the Activity Log; the CLI form `fid program check` (needs `AUTO.CLI.DISPATCH`) and the MCP tool. So AC-1's `fid program check --program root:in/clean.json --json` invocation and AC-6's "Record Journal unchanged" half are not met; AC-1 to AC-6 are otherwise tested on the library (`crates/fid-program/tests/check.rs`).
- **AC-3 wording.** "missing `offset` ... `offset: 5[deg]`" names one parameter as both missing and passed. The fixture reads it as three different offending parameters: required `axis` missing, unknown `colour`, and `offset: 5[deg]` (an angle for a length). Suggested erratum: "missing `axis`".
- **AC fixture path.** The `demo.*` commands are added to `fixtures/manifest/prog-demo.json` through its generator, `fixtures/program/types/make_fixtures.py` (owned by `AUTO.PROG.EXPR.TYPES`); no TYPES or FLOW expected output changes.
- **Choices pinned in the module docs** (stated in the PR): a `hidden` major and an unreadable lifecycle state are `unknown_command` like `removed` (REQ-001 names only absent and removed); entity references in literals are checked for JSON shape only (existence is run-time state); a literal whose property schema cannot be compiled on its own is `bad_arguments` reason `unchecked` (fail closed); `program_class` is `null` when a reachable invoke does not resolve (no class is claimed for an unknown command); each step carries `reachable`, and each diagnostic `slot` and `names`, beyond the Design's fields; the command takes `program` (read-root handle) or `program_id`, not one `ProgramRef` string; an agent audience gets the registry's agent view (D-035), so a never-projected command is `unknown_command` to it.
- **`backward_jump` fixture pending.** Once `AUTO.PROG.FLOW` implements REQ-AUTO.PROG.FLOW-010 (a jump to a label at or before it is `backward_jump`), add a CHECK fixture with a backward jump so the checker's result carries it in order with the other modules.
- **Fail-closed argument rules added in review of #637.** An expression for a parameter with no static type (a `$ref`, a union) that is not a value of record is `unchecked`; on an all-literal call the whole arguments object is validated against the root `params_schema`, and with an expression argument a root keyword beyond `type`, `properties`, `required`, `additionalProperties` (and the non-asserting `$defs`, `$schema`, `$id`, `$comment`, `title`, `description`) makes `(arguments)` `unchecked`. Every module sees only the callable (active or deprecated) majors, so a hidden, removed or unreadable major gives byte-identical diagnostics to an absent one.
- **Overlap with `AUTO.PROG.EXPR.TYPES`.** An absent command is also `type_unknown` at `command`, and an expression argument of the wrong type is also `type_mismatch`; the checker keeps every module's diagnostics as they are and adds its own.

## Children

_Leaf: none._
