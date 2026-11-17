---
id: TRUST.PROFILE.GATE.ASSIST
title: Assistive-only agent-write bus hook
level: 4
parent: TRUST.PROFILE.GATE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE.GATE-003]
covers: []
depends_on: [TRUST.PROFILE.GATE.QUERY, CORE.BUS.POL.CLASS, CORE.BUS.PRIN, CORE.REG.DEF]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PROFILE.GATE.ASSIST: Assistive-only agent-write bus hook

## Purpose

While the Assistive-only profile is active, agents may only propose. This leaf is the bus policy hook that refuses W1 and W2 commands from `agent` principals, except the commands that create or update a proposal on that agent's own branch. It narrows what agents may do; it never adds, removes or reclassifies a command (REQ-TRUST.PROFILE-003).

## Scope

**In:**
- A `PolicyHook` implementation called by the bus at its authorise stage
- The allowlist of proposal commands, read from registry metadata
- The refusal `profile.agent_write_forbidden`

**Out:**
- The fixed class/principal matrix (`CORE.BUS.POL.CLASS`)
- Agent branches and proposals themselves (`CORE.JRN.BR`, `AGENT.PROPOSE`)
- Reading the profile (`TRUST.PROFILE.GATE.QUERY`)

## Requirements

- **REQ-TRUST.PROFILE.GATE.ASSIST-001** While Assistive-only is active, the hook shall refuse every W1 or W2 command invoked by an `agent` principal with `profile.agent_write_forbidden`, naming the command ID, before the handler runs.
- **REQ-TRUST.PROFILE.GATE.ASSIST-002** The hook shall allow a W1 or W2 agent command whose registry definition carries the `proposal` capability tag and whose target branch is the invoking agent's own branch.
- **REQ-TRUST.PROFILE.GATE.ASSIST-003** If a `proposal`-tagged command targets a branch other than the invoking agent's own, then the hook shall refuse it with `profile.agent_write_forbidden`.
- **REQ-TRUST.PROFILE.GATE.ASSIST-004** While Lean or Regulated is active, and for every non-agent principal, the hook shall return `Continue` without effect.
- **REQ-TRUST.PROFILE.GATE.ASSIST-005** The hook shall only return `Continue` or `Refuse`; it shall never change a command's class, parameters or projection.

## Acceptance criteria

- **AC-1** Given fixture registry `fixtures/profile/assist/registry` (one W2 edit, one W1 draft, one `proposal`-tagged W2, one R1) and Assistive-only active, when an agent principal invokes each on its own branch, then the W2 edit and W1 draft are refused with `profile.agent_write_forbidden`, the proposal and the R1 run, and nothing is journaled for refusals.
- **AC-2** (error path) Given the proposal command targeting `main`, then it is refused.
- **AC-3** Given Lean active, or a `human` principal under Assistive-only, then all four commands pass the hook.
- **AC-4** Given the registry manifest digest before and after enabling the hook, then it is unchanged.

## Design

- **Registry command(s):** none; registers a `PolicyHook` with the bus.
- **Data structures:** `HookDecision = Continue | Refuse(BusError)`; capability tag `proposal` in the command definition (CORE.REG.DEF tags) [unverified: tag mechanism name].
- **Algorithm:** if profile is not Assistive-only or principal is not `agent` → Continue; if class not in {W1, W2} → Continue (the CORE matrix already forbids M1/S1/W3/A0 for agents); if definition has `proposal` and target branch == agent's branch → Continue; else Refuse.
- **Errors:** `profile.agent_write_forbidden` mapped to bus error `policy` with that reason.
- **Determinism:** D0.
- **Performance budget:** < 2 µs per call [unverified until measured].
- **Crate:** `fid-profile`, module `assist` (links `fid-bus` policy hook trait).

## Test fixtures

A test registry built in code (`fixtures/profile/assist/registry`) with four commands as in AC-1.

## Build prompt

> In crate `fid-profile`, add module `assist` implementing the bus `PolicyHook` trait (authorise stage). Using a `ProfileSnapshot` from module `query`: when the profile is Assistive-only and the principal kind is `agent`, refuse W1 and W2 commands with bus error `policy`, reason `profile.agent_write_forbidden`, naming the command, unless the command definition has capability tag `proposal` and its target branch is the invoking agent's own branch. In every other case return `Continue`. Never alter class, parameters or projection. Tests: AC-1..AC-4. Constraints: CONSTITUTION §3 (profiles never change the class taxonomy), §8, D0. If CORE.BUS has no hook trait yet, define the trait signature in an adapter and record the gap.

## Open questions

- CORE.BUS.POL.CLASS has no extension hook today; CORE.BUS must add a `PolicyHook` call at the authorise stage [cross-domain, unresolved].
- The `proposal` capability tag must be declarable in CORE.REG.DEF and set by AGENT.PROPOSE commands [unverified].

## Children

_Leaf: none._
