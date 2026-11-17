---
id: AUTO.PROG.EDIT.VIEWS
title: Command view, summary view and program.show
level: 4
parent: AUTO.PROG.EDIT
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PROG.EDIT-003, REQ-AUTO.PROG.EDIT-004]
covers: [PCD.PRG.01]
depends_on: [AUTO.PROG.FMT, AUTO.PROG.CHECK, AUTO.REC.ADOPT, CORE.REG]
side_effect: R0
determinism: D0
cli: [fid program show]
mcp: [program_show]
journal: [program.show@1]
updated: 2026-11-17
---

# AUTO.PROG.EDIT.VIEWS: Command view, summary view and program.show

## Purpose

The two read models every program editor shows. The command view is one row per step with its full arguments, in document order. The summary view groups steps into readable blocks (for example "Align to datum system A-B-C", "Measure 4 holes", "Ask operator for gap") whose titles come from registry help texts and author comments. Both are pure functions of the canonical program plus the manifest, keyed by step ID so a selection in one view maps to the other, and both show each step's diagnostics and agent-provenance tag. `program.show@1` (R0) returns either view.

## Scope

**In:**
- Command-view rows: step ID, depth, kind, command and title, arguments (literal or expression text), bound variable, diagnostics, provenance
- Summary-view blocks: grouping rules, generated titles, nesting for control flow and subroutines, step-ID membership
- Bidirectional mapping by step ID between the views
- `program.show@1` (R0) with `view: command | summary`, paging and a stable order

**Out:**
- Edits (`AUTO.PROG.EDIT.OPS`, `AUTO.PROG.EDIT.REFACT`)
- Widgets and rendering (`VIZ`)
- A text or Python view (not in P1b, D-026)

## Requirements

- **REQ-AUTO.PROG.EDIT.VIEWS-001** Both views shall be computed only from the canonical program bytes, the manifest, the check diagnostics and the provenance store, so the same inputs give byte-identical views.
- **REQ-AUTO.PROG.EDIT.VIEWS-002** The set of step IDs in the command view and the union of step IDs over all summary blocks shall be equal, and every step ID shall belong to exactly one innermost summary block.
- **REQ-AUTO.PROG.EDIT.VIEWS-003** The summary view shall start a new block at every comment step whose text begins with `##`, at every operator step, and whenever the command domain or the primary element argument changes between consecutive `invoke` steps; control-flow steps and subroutines shall form nested blocks.
- **REQ-AUTO.PROG.EDIT.VIEWS-004** Each block title shall be the `##` comment text if present, otherwise a title built from fixed project-authored templates and the registry titles of its commands, and shall contain no text taken from document content other than author comments and element names, both marked untrusted.
- **REQ-AUTO.PROG.EDIT.VIEWS-005** Every row and every block containing an unadopted agent-authored step shall carry the provenance tag naming the agent, as reported by `unadopted_steps`.
- **REQ-AUTO.PROG.EDIT.VIEWS-006** `program.show@1` shall be R0 and return pages of at most 500 rows or blocks with a continuation cursor.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/program/edit/macro.json` and manifest `fixtures/manifest/prog-demo.json`, when `fid program show --program P --view command --json` and `--view summary --json` run, then the outputs equal `macro.command.json` and `macro.summary.json` byte for byte.
- **AC-2** Given both outputs, then the command-view step-ID set equals the union of summary-block step IDs and each ID appears in exactly one innermost block.
- **AC-3** Given a draft where an agent changed step S7, then row S7 and its enclosing block carry the agent's provenance tag; after a human adopts S7, neither does.
- **AC-4** Given a program with a type error in S4, then row S4 lists the diagnostic code and the block containing S4 is marked as having errors.
- **AC-5** Given a 1 200-step program, then the first page holds 500 rows and a cursor, and three pages cover all rows in order.

## Design

- **Registry command(s):** `program.show@1`, R0. Params: `program: ProgramRef`, `view: Command | Summary`, `cursor: Option<Cursor>`. Result: `CommandView { rows, next }` or `SummaryView { blocks, next }`.
- **Data structures:** `Row { step_id, depth, kind, command, title, args: Vec<ArgView>, bind, diagnostics, provenance }`; `Block { block_id: derived UUIDv5 of first step ID and rule, title: Untrusted-aware text, kind, step_ids, children: Vec<Block>, has_errors, provenance }`.
- **Algorithm:** one ordered walk; block boundaries by REQ-003; titles from templates keyed by command domain; block IDs derived (UUIDv5) so they are stable while their first step survives.
- **Errors:** `program_not_found`, `bad_cursor`.
- **Determinism:** D0.
- **Performance budget:** both views for 10 000 steps < 100 ms [unverified until measured].
- **Crate:** `fid-program` (module `edit::views`).

## Test fixtures

`fixtures/program/edit/macro.json` with expected `macro.command.json` and `macro.summary.json`; provenance fixture records; `fixtures/manifest/prog-demo.json`. Project-authored, CC-BY-4.0.

## Build prompt

> In crate `fid-program`, add module `edit::views` and register `program.show@1` (R0; CLI `fid program show`, MCP `program_show`). Command view: one row per step in document order (step ID, depth, kind, command and registry title, arguments as literal JSON or expression text, bound variable, diagnostics from `check_program`, provenance from `adopt::unadopted_steps`). Summary view: nested blocks; a new block at each `##` comment, each operator step, and each change of command domain or primary element argument between consecutive invokes; control flow and subroutines nest. Titles from the `##` comment or fixed project-authored templates plus registry titles; comment text and element names are untrusted. Block IDs are UUIDv5 of first step ID and rule. Both views are pure functions of program bytes, manifest, diagnostics and provenance; step-ID sets must match. Pages of 500 with a cursor. Acceptance: AC-1..AC-5 with golden files. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; C6 for content strings; reads journal only to the Activity Log (D-031).

## Open questions

- The grouping rule in REQ-003 is a first proposal; whether users of incumbent editors expect grouping per measured feature regardless of domain is [unverified] and may change the rule (not the contract).

## Children

_Leaf: none._
