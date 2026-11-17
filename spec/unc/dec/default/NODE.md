---
id: UNC.DEC.DEFAULT
title: Rule resolution, dialect defaults and decide queries
level: 3
parent: UNC.DEC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.DEC-001, REQ-UNC.DEC-003, REQ-UNC.DEC-004, REQ-UNC.DEC-005]
covers: []
depends_on: [UNC.DEC.TYPE, UNC.DEC.EVAL, UNC.DEC.RISK, CORE]
side_effect: R1
determinism: D0
cli: [fid decide evaluate-rule, fid decide list-rule, fid decide explain-decision]
mcp: [decide_evaluate_rule, decide_list_rule, decide_explain_decision]
journal: [decide.evaluate_rule@1, decide.list_rule@1, decide.explain_decision@1]
updated: 2026-11-16
---

# UNC.DEC.DEFAULT: Rule resolution, dialect defaults and decide queries

## Purpose

Decides which rule applies to a characteristic and exposes the decision engine as commands. Resolution order is explicit and recorded: a characteristic binding, then a plan default, then a tenant default, then the dialect default. Under ISO GPS the dialect default is the ISO 14253-1 rule; under ASME there is no implicit default, so an unbound ASME characteristic is NOT_DECIDABLE with reason `rule_missing`.

## Scope

**In:**
- Resolution function and its record (`resolved_from: char_binding | plan_default | tenant_default | dialect_default`)
- Dialect defaults table `dr-dialect-defaults@1`: ISO GPS -> `dr:iso14253-1:2017/default@1`; ASME -> none
- Transcribed results (evaluation basis `RESULTS_ONLY` with a source status) -> `dr:as-reported-by-source@1` label only
- `decide.evaluate_rule@1` (R1), `decide.list_rule@1` (R0), `decide.explain_decision@1` (R0)

**Out:**
- Writing bindings and tenant defaults (owned by `UNC.DEC.BIND`)
- Choosing the dialect of a characteristic (owned by `GDT`)
- Committing the decision into the ResultRecord (owned by the result producer in `GDT`)

## Requirements

- **REQ-UNC.DEC.DEFAULT-001** The system shall resolve the rule for a characteristic in the order: characteristic binding, plan default, tenant default, dialect default, and record which level supplied it.
- **REQ-UNC.DEC.DEFAULT-002** While a characteristic is under ISO GPS and no binding or default applies, the system shall resolve `dr:iso14253-1:2017/default@1`.
- **REQ-UNC.DEC.DEFAULT-003** If a characteristic is under ASME and no characteristic binding, plan default or tenant default exists, then `decide.evaluate_rule@1` shall return `NOT_DECIDABLE(rule_missing)` and shall not fall back to any rule.
- **REQ-UNC.DEC.DEFAULT-004** Every decision record returned by `decide.evaluate_rule@1` shall carry the rule ID and version and the resolution level.
- **REQ-UNC.DEC.DEFAULT-005** When `decide.explain_decision@1` is called on a result, the system shall return the rule, its resolution level, the inputs used (value, U, p, limits), the outcome, risk figures if any, and for NOT_DECIDABLE the reason with a suggested fix (`decide.bind_rule@1` for `rule_missing`, `unc.propagate_auto@1` for `u_missing`).
- **REQ-UNC.DEC.DEFAULT-006** When `decide.list_rule@1` is called, the system shall return the built-in catalog and the tenant's customer rules with IDs, versions, kinds and parameters.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dec/default/two_dialects.fidproj` with an ISO GPS flatness `F-1` and an ASME position `P-1`, both unbound and with UEC-2 statements, when `fid decide evaluate-rule --result F-1 --json` runs, then the rule is `dr:iso14253-1:2017/default@1` with `resolved_from = dialect_default`; for `P-1` the outcome is `NOT_DECIDABLE(rule_missing)`.
- **AC-2** Given the same project with a tenant default `dr:simple-acceptance@1`, when `P-1` is evaluated, then it is decided under simple acceptance with `resolved_from = tenant_default`.
- **AC-3** Given a characteristic binding `dr:guard-band(r=1)@1` on `F-1`, then it overrides the dialect default and `resolved_from = char_binding`.
- **AC-4** Given `P-1` from AC-1, when `fid decide explain-decision --result P-1 --json` runs, then the reason is `rule_missing` and the fix names `decide.bind_rule@1`.
- **AC-5** Given a transcribed result with basis `RESULTS_ONLY` and source status "OK", then the record uses `dr:as-reported-by-source@1` with `NOT_DECIDABLE(label_only)` and the source status as a label.
- **AC-6** (replay) Given a journal of the calls above, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `decide.evaluate_rule@1` (R1; `result_id`, optional `rule` override for what-if, `risk: bool`; returns `DecisionRecord` plus `resolved_from`), `decide.list_rule@1` (R0; optional `kind` filter), `decide.explain_decision@1` (R0; `result_id`).
- **Data structures:** `Resolution { rule: Option<RuleRef>, resolved_from }`; dialect defaults as a versioned data file.
- **Algorithm:** read bindings and defaults from the plan and tenant settings; pick by order; call kernel `dec.rule.evaluate@1.0.0` (and `dec.risk.compute@1.0.0` when requested) through the bus; return the record. A what-if override is labelled `what_if` and never stored.
- **Errors:** `result_not_found`, `rule_not_found`.
- **Determinism:** D0.
- **Performance budget:** under 2 ms per evaluation excluding kernel time [unverified until measured].
- **Crate:** `fid-dec`.

## Test fixtures

`fixtures/unc/dec/default/two_dialects.fidproj`: synthetic project built by a test helper (CC-BY-4.0) with statements taken from `fixtures/unc/stmt/valid/`.

## Build prompt

> In crate `fid-dec`, implement rule resolution and three registry commands: `decide.evaluate_rule@1` (R1; CLI `fid decide evaluate-rule`; MCP `decide_evaluate_rule`), `decide.list_rule@1` (R0) and `decide.explain_decision@1` (R0), each journaled with command ID and result digest, all with `--json`.
> Resolution order: characteristic binding, plan default, tenant default, dialect default; record `resolved_from`. Dialect defaults (versioned data `dr-dialect-defaults@1`): ISO GPS -> `dr:iso14253-1:2017/default@1`; ASME -> none, giving `NOT_DECIDABLE(rule_missing)` with no fallback. Transcribed results (basis `RESULTS_ONLY` with source status) -> `dr:as-reported-by-source@1`, `NOT_DECIDABLE(label_only)`, source status kept as a label.
> evaluate_rule calls the kernel algorithms `dec.rule.evaluate@1.0.0` and optionally `dec.risk.compute@1.0.0` from `fid-kdec` through the bus; it returns the record with rule ID, version and resolution level; a `--rule` override is a what-if, labelled and never stored. explain_decision returns rule, level, inputs (value, U, p, limits), outcome, risk, and for NOT_DECIDABLE the reason plus a fix (`decide.bind_rule@1` for `rule_missing`, `unc.propagate_auto@1` for `u_missing`). list_rule returns the built-in catalog plus tenant customer rules.
> Tests with `fixtures/unc/dec/default/two_dialects.fidproj`: ISO default applied; ASME unbound is rule_missing; tenant default and characteristic binding override; explanation fix; transcription label; exact replay IDENTICAL. No panics, no `unwrap`.

## Open questions

- Current edition and reaffirmation status of ASME B89.7.3.1 (which concerns stating decision rules) needs confirmation [unverified]; it does not change the "no implicit ASME default" behaviour.

## Children

_Leaf: none._
