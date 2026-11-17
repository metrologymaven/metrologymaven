---
id: FLOW.DEADEND.RESULT
title: Suggested-fix resolution per principal and result projection contract
level: 3
parent: FLOW.DEADEND
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-FLOW.DEADEND-003, REQ-FLOW.DEADEND-004]
covers: [JOB.USE.05]
depends_on: [FLOW.DEADEND.CATALOG, CORE.BUS.PRIN, CORE.BUS.POL, CORE.REG.MAN]
uses_artifacts: [AUTO.SURF.ENV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# FLOW.DEADEND.RESULT: Suggested-fix resolution per principal and result projection contract

## Purpose

Turns a reason code plus its context into the concrete list of suggested fixes a result carries: command IDs with pre-filled arguments, each marked for whether the current principal may run it. It also publishes the `suggested_fixes` JSON shape that CLI `--json` output and MCP results embed, and a conformance test that the generated surfaces carry it. Fixes a principal cannot run are listed and marked, never hidden.

## Scope

**In:**
- `resolve_fixes(reason_code, context, principal) -> Vec<SuggestedFix>` in `fid-fixes`
- Argument pre-filling from the closed placeholder set
- `requires_human` marking from side-effect class (M1, S1, W3, A0), `human_only` and MCP projection
- The published `suggested-fixes-v1.json` schema
- A surface conformance test fixture (one fixture command emitting a catalogued reason)

**Out:**
- The catalogue (owned by `FLOW.DEADEND.CATALOG`)
- The envelope field itself and its encoding in CLI and MCP (owned by `AUTO.SURF.ENV`, `AUTO.CLI`, `AUTO.MCP`)
- GUI presentation of fixes (owned by `VIZ.SHELL`; reachability is checked by `FLOW.DEADEND.WHY`)

## Requirements

- **REQ-FLOW.DEADEND.RESULT-001** When `resolve_fixes` is called with a catalogued reason code, it shall return every catalogue fix for that code in catalogue order, with each argument placeholder replaced by the matching context value.
- **REQ-FLOW.DEADEND.RESULT-002** If a placeholder has no value in the context, then the returned fix shall omit that argument and set `incomplete_args: true`, rather than drop the fix.
- **REQ-FLOW.DEADEND.RESULT-003** If a fix's command has side-effect class M1, S1, W3 or A0, is declared `human_only`, or is request-only for the given principal, then the returned fix shall carry `requires_human: true` and remain in the list.
- **REQ-FLOW.DEADEND.RESULT-004** If the reason code is not catalogued, then `resolve_fixes` shall return an empty list and a `not_catalogued` flag, and shall not fail.
- **REQ-FLOW.DEADEND.RESULT-005** The published `suggested-fixes-v1.json` schema shall describe an array of `{command, args, requires_human, incomplete_args, label_key}` objects serialised as RFC 8785 canonical JSON.

## Acceptance criteria

- **AC-1** Given catalogue fixture `catalogue-ok.json` and context `{characteristic_id: "c-1", dialect: "asme"}`, when `resolve_fixes("decide.rule_missing", ctx, human)` runs, then it returns `decide.bind_rule@1` then `decide.set_default_rule@1` with `characteristic_id = "c-1"` and `requires_human: false`.
- **AC-2** Given an `agent` principal and fixture manifest where `decide.set_default_rule@1` is `human_only`, when resolved, then that fix is present with `requires_human: true`.
- **AC-3** (error path) Given reason `scan.unknown`, then the result is empty with `not_catalogued: true`.
- **AC-4** Given a fixture command emitting `decide.rule_missing` through the generated CLI and MCP surfaces, when invoked with `--json` and over stdio MCP, then both results contain an array valid against `suggested-fixes-v1.json` (runs once `AUTO.SURF.ENV` carries the field).

## Design

- **Registry command(s):** none (library used by surfaces and by result-producing commands).
- **Data structures:** `SuggestedFix { command, args: BTreeMap<String, Value>, requires_human: bool, incomplete_args: bool, label_key }`; `FixContext` with the five optional placeholder values; `PrincipalView { kind: human|script|agent, channel }` from `CORE.BUS.PRIN`.
- **Algorithm:** catalogue lookup; template substitution; class and `human_only` lookup in the manifest; projection rule from `CORE.BUS.POL` for the principal.
- **Errors:** none returned to callers (fail-soft); internal invariant violations logged to the Activity Log.
- **Determinism:** D0.
- **Performance budget:** under 50 µs per call for 5 fixes [unverified].
- **Crate:** `fid-fixes`.

## Test fixtures

Fixture manifest and catalogue under `fixtures/flow/fixes/`; a test-only fixture command registered in the surface conformance harness.

## Build prompt

> In crate `fid-fixes`, add `resolve_fixes(reason_code: &str, ctx: &FixContext, principal: &PrincipalView, manifest: &Manifest) -> Resolution { fixes: Vec<SuggestedFix>, not_catalogued: bool }`. Substitute placeholders `${entity_id}`, `${characteristic_id}`, `${project_id}`, `${dialect}`, `${reason_detail}`; missing values drop the argument and set `incomplete_args`. Mark `requires_human` when the command's class is M1/S1/W3/A0, it is `human_only`, or the bus projection policy makes it request-only for the principal; never remove a fix. Generate and check in `schema/suggested-fixes-v1.json`. Tests for AC-1..AC-3 of spec node FLOW.DEADEND.RESULT; AC-4 as an ignored integration test until the envelope field exists. CONSTITUTION §8: no panic, no `unsafe`, no `unwrap`; ordered maps only.

## Open questions

- REQ-FLOW.DEADEND-003 needs the shared result envelope (`AUTO.SURF.ENV`) to carry an optional `suggested_fixes` field that surfaces fill by calling this library. That field is not in REQ-AUTO.SURF.ENV-001 today; it is a cross-domain change for AUTO.

## Children

_Leaf: none._
