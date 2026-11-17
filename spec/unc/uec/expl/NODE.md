---
id: UNC.UEC.EXPL
title: Show and explain uncertainty queries
level: 3
parent: UNC.UEC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.UEC-003]
covers: []
depends_on: [UNC.UEC.STMT, CORE]
side_effect: R0
determinism: D0
cli: [fid unc show-statement, fid unc explain-result]
mcp: [unc_show_statement, unc_explain_result]
journal: [unc.show_statement@1, unc.explain_result@1]
updated: 2026-11-16
---

# UNC.UEC.EXPL: Show and explain uncertainty queries

## Purpose

Read-only queries that answer "what is this result's uncertainty and why": the full statement, its contributors ranked by variance share, sensitivities, the LPU-versus-MCM comparison flag, and, for UEC-0, the "not evaluated" label with a typed reason and suggested fixes. The viewer's clickable class badge, RPT and MCP agents all use these two commands, so the wording and the fix catalogue are defined once.

## Scope

**In:**
- `unc.show_statement@1`: the stored statement, rendered as structured data
- `unc.explain_result@1`: label, class meaning, top contributors, method facts, and for UEC-0 the reason and suggested fixes
- The suggested-fix catalogue `uec-fix@1` mapping each UEC-0 reason to one or more fix actions (command IDs)

**Out:**
- Computing contributors and sensitivities (owned by `UNC.PROP.RANK`; this node reads what the statement stores)
- Display widgets (owned by `VIZ`) and report layout (owned by `RPT`)

## Requirements

- **REQ-UNC.UEC.EXPL-001** When `unc.explain_result@1` is called on a UEC-0 result, the system shall return the label "not evaluated", the stored reason (`u_missing`, `model_missing`, `instrument_missing`) and at least one suggested fix naming a registry command ID.
- **REQ-UNC.UEC.EXPL-002** When `unc.explain_result@1` is called on a UEC-1..3 result, the system shall return the class meaning, method, coverage, model reference and the contributors stored in the statement, ordered by descending variance share and limited to a `top` parameter (default 5).
- **REQ-UNC.UEC.EXPL-003** The system shall never return a numeric uncertainty value for a UEC-0 result from either command.
- **REQ-UNC.UEC.EXPL-004** The suggested-fix catalogue shall map `model_missing` to `unc.bind_model@1`, `instrument_missing` to `unc.bind_instrument_model@1` or `unc.list_profile@1`, and `u_missing` to `unc.propagate_auto@1` or `unc.declare_uncertainty@1`.
- **REQ-UNC.UEC.EXPL-005** When the result ID does not exist, both commands shall fail with `result_not_found`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/unc/expl/mixed.fidproj` with results R0 (UEC-0, `instrument_missing`), R1 (UEC-1 declared) and R2 (UEC-2 MCM with 8 contributors), when `fid unc explain-result --result R0 --json` runs, then the output has `label = "not evaluated"`, `reason = "instrument_missing"`, a fix list containing `unc.bind_instrument_model@1`, and no numeric field named `U`, `u_std` or `interval`.
- **AC-2** Given R2, when `fid unc explain-result --result R2 --top 3 --json` runs, then exactly 3 contributors are returned in descending variance share and their shares equal the stored values bit for bit.
- **AC-3** Given R1, when `fid unc show-statement --result R1 --json` runs, then the output equals the stored statement's canonical JSON.
- **AC-4** (error path) Given result ID `R9`, when either command runs, then it fails with `result_not_found`.
- **AC-5** (replay) Given a journal containing the calls from AC-1..3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.show_statement@1` (R0; `result_id`; returns the statement) and `unc.explain_result@1` (R0; `result_id`, `top: u32 = 5`; returns `Explanation`).
- **Data structures:** `Explanation { label, class, class_meaning, method_facts: Option<{method, k_or_interval, model, lpu_validated_by_mc, nonlinearity}>, contributors: Vec<Contributor>, uec0: Option<{reason, fixes: Vec<FixAction{command_id, hint_key}>}> }`. Text is returned as message keys plus parameters, not prose, so surfaces localize it.
- **Algorithm:** load the result, read its statement, sort contributors by descending `variance_share` with ties broken by `input_id`, truncate to `top`, look up fixes in the static catalogue `uec-fix@1`.
- **Errors:** `result_not_found`, `top_out_of_range` (top must be 1..100).
- **Determinism:** D0 (read and sort only).
- **Performance budget:** under 5 ms per call on a 10,000-result project [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

`fixtures/unc/expl/mixed.fidproj`: a synthetic project built by a test helper with three results whose statements are fixed JSON from `fixtures/unc/stmt/valid/` (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, implement two R0 registry commands: `unc.show_statement@1` (CLI `fid unc show-statement --result <id>`, MCP `unc_show_statement`) returning the stored `UncertaintyStatement` canonical JSON, and `unc.explain_result@1` (CLI `fid unc explain-result --result <id> [--top N]`, MCP `unc_explain_result`) returning an `Explanation`: label, class and class-meaning key, method facts (method, k or interval, model ref, `lpu_validated_by_mc`, nonlinearity outcome), contributors sorted by descending variance share (ties by `input_id`) truncated to `top` (default 5, range 1..100), and for UEC-0 the stored reason plus fixes from a static catalogue `uec-fix@1`: `model_missing` -> `unc.bind_model@1`; `instrument_missing` -> `unc.bind_instrument_model@1`, `unc.list_profile@1`; `u_missing` -> `unc.propagate_auto@1`, `unc.declare_uncertainty@1`.
> Never output a numeric uncertainty for UEC-0. Return message keys, not prose. Errors: `result_not_found`, `top_out_of_range`. Both commands are journaled per CONSTITUTION C4 (events `unc.show_statement@1`, `unc.explain_result@1`) and support `--json`.
> Tests with `fixtures/unc/expl/mixed.fidproj`: UEC-0 explanation has label, reason, fix and no numbers; top-3 contributors are ordered and bit-equal to stored values; show-statement equals stored canonical JSON; unknown ID fails; exact replay is IDENTICAL. No panics, no `unwrap`.

## Open questions

- None.

## Children

_Leaf: none._
