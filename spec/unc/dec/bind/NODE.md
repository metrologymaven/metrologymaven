---
id: UNC.DEC.BIND
title: Rule binding, change audit and adoption
level: 3
parent: UNC.DEC
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.DEC-008, REQ-UNC.DEC-009]
covers: []
depends_on: [UNC.DEC.TYPE, CORE]
side_effect: W2
determinism: D0
cli: [fid decide bind-rule, fid decide change-rule, fid decide adopt-rule, fid decide set-default-rule]
mcp: [decide_bind_rule, decide_change_rule]
journal: [decide.bind_rule@1, decide.change_rule@1, decide.adopt_rule@1, decide.set_default_rule@1]
updated: 2026-11-16
---

# UNC.DEC.BIND: Rule binding, change audit and adoption

## Purpose

The write path for decision rules. Binding a rule to characteristics or a plan is part of the plan; changing a bound rule after results exist creates a conspicuous journal event that marks every affected result "rule changed after measurement", which deters rule shopping. Agent-chosen rules are proposals until a human adopts them. Tenant defaults are administrative configuration.

## Scope

**In:**
- `decide.bind_rule@1` (W2): bind a rule to characteristics or as the plan default, only while no results exist under the previous resolution
- `decide.change_rule@1` (W2): change a binding when results exist; mark affected results
- `decide.adopt_rule@1` (W2, human only, not on MCP): adopt an agent-proposed binding
- `decide.set_default_rule@1` (A0, never on MCP): set the tenant default rule
- The `rule_changed_after_measurement` mark on results and its reason text (untrusted)

**Out:**
- Resolution and evaluation (owned by `UNC.DEC.DEFAULT` and `UNC.DEC.EVAL`)
- Showing the mark on reports (owned by `RPT`)
- Release gating (owned by `TRUST`), which reads the proposal flag

## Requirements

- **REQ-UNC.DEC.BIND-001** When `decide.bind_rule@1` runs on characteristics with no existing results under their current resolution, the system shall store the binding with rule ID and version and commit one journal event.
- **REQ-UNC.DEC.BIND-002** If any affected characteristic already has a result, then `decide.bind_rule@1` shall fail with `results_exist_use_change_rule` and commit nothing.
- **REQ-UNC.DEC.BIND-003** When `decide.change_rule@1` runs, the system shall require a reason, store the new binding, mark every existing result under the old resolution with `rule_changed_after_measurement` (old rule, new rule, reason, principal, journal time) and commit all of it as one journal event.
- **REQ-UNC.DEC.BIND-004** When an agent principal binds or changes a rule, the system shall mark the binding `agent_proposal`, and results evaluated under it shall not be releasable until a human runs `decide.adopt_rule@1` on that binding.
- **REQ-UNC.DEC.BIND-005** If `decide.adopt_rule@1` or `decide.set_default_rule@1` is called by an agent principal, then it shall fail with `principal_not_human`; neither shall be projected to MCP.
- **REQ-UNC.DEC.BIND-006** The `decide.set_default_rule@1` command shall be side-effect class A0 and shall be journaled with the previous and new tenant default.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/dec/bind/plate.fidproj` (three characteristics, no results), when `fid decide bind-rule --chars C-1,C-2 --rule dr:simple-acceptance@1 --json` runs as a human, then both bindings exist and one `decide.bind_rule@1` event is journaled.
- **AC-2** (error path) Given C-1 now has a result, when `bind-rule` runs again on C-1, then it fails with `results_exist_use_change_rule` and the journal head is unchanged.
- **AC-3** (change) Given C-1 and C-2 each with a result, when `fid decide change-rule --chars C-1,C-2 --rule dr:guard-band(r=1)@1 --reason "customer request"` runs, then both results carry `rule_changed_after_measurement` with old and new rule, and exactly one `decide.change_rule@1` event is journaled.
- **AC-4** (error path) Given `change-rule` without `--reason`, then it fails with `reason_required`.
- **AC-5** (agent) Given an MCP `decide_bind_rule` call by an agent, then the binding is `agent_proposal` on the agent branch; the release check reports not releasable until a human runs `fid decide adopt-rule --binding <id>`.
- **AC-6** (MCP surface) Given the generated MCP tool list, then it contains `decide_bind_rule` and `decide_change_rule` and contains neither `decide_adopt_rule` nor `decide_set_default_rule`.
- **AC-7** (replay) Given the journal from AC-1..3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `decide.bind_rule@1` (W2; `chars: Vec<EntityId> | plan_default`, `rule: RuleRef`), `decide.change_rule@1` (W2; same plus `reason: UntrustedString(≤2000)`), `decide.adopt_rule@1` (W2, registry flag `human_only`; `binding_id`), `decide.set_default_rule@1` (A0; `rule: RuleRef`).
- **Data structures:** `RuleBinding { binding_id (UUIDv7), target, rule, provenance { author_kind, adopted_by } }`; `RuleChangeMark { old_rule, new_rule, reason, principal, at }` stored on results.
- **Algorithm:** validate rule against the catalog; query results under the target's current resolution in stable ID order; build all marks; commit atomically through the bus.
- **Errors:** `rule_not_found`, `char_not_found`, `results_exist_use_change_rule`, `reason_required`, `principal_not_human`.
- **Determinism:** D0.
- **Performance budget:** change-rule marking 10,000 results in under 200 ms excluding fsync [unverified until measured].
- **Crate:** `fid-dec`.

## Test fixtures

`fixtures/unc/dec/bind/plate.fidproj`: synthetic project from a test helper (CC-BY-4.0).

## Build prompt

> In crate `fid-dec`, implement four registry commands, each journaled with command ID and result digest: `decide.bind_rule@1` (W2; CLI `fid decide bind-rule`; MCP `decide_bind_rule`), `decide.change_rule@1` (W2; `fid decide change-rule`; `decide_change_rule`), `decide.adopt_rule@1` (W2 with registry flag `human_only`; CLI only) and `decide.set_default_rule@1` (side-effect class A0; never on MCP; CLI only).
> bind_rule: validate the rule against the `fid-kdec` catalog; fail `results_exist_use_change_rule` if any target already has a result; else store `RuleBinding` and commit one event. change_rule: require an untrusted `reason` (≤ 2,000 chars, else `reason_required`); store the new binding and mark every existing result under the old resolution with `rule_changed_after_measurement {old_rule, new_rule, reason, principal, at}` in one atomic commit. Agent principals: bindings are `agent_proposal`, not releasable until a human adopts (CONSTITUTION C5); adopt and set_default reject agents with `principal_not_human`. set_default_rule journals previous and new tenant defaults.
> Tests with `fixtures/unc/dec/bind/plate.fidproj`: bind; bind refused once results exist; change marks both results in one event; missing reason; agent proposal then human adoption; generated MCP list excludes adopt and set-default; exact replay IDENTICAL. Support `--json` and `--dry-run`. No panics, no `unwrap`.

## Open questions

- Classing `decide.set_default_rule@1` as A0 (admin configuration) follows CONSTITUTION §3; if the steering group prefers W3, only the class changes.

## Children

_Leaf: none._
