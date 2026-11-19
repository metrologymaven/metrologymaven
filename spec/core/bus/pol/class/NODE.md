---
id: CORE.BUS.POL.CLASS
title: Class, principal and human-only matrix
level: 4
parent: CORE.BUS.POL
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-bus]
built_pr: 280
satisfies: [REQ-CORE.BUS.POL-001, REQ-CORE.BUS.POL-009]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# CORE.BUS.POL.CLASS: Class, principal and human-only matrix

## Purpose

The first check of the authorise stage: a fixed table from (principal kind, side-effect class, `human_only` flag) to allow or refuse. It encodes the CONSTITUTION §3 bans (agents never perform M1, S1, W3 or A0), the D-031 `human_only` rule (agents and scripts refused whatever the class), and the §3.1 job-cancel rule (own jobs, or a human).

## Scope

**In:** the class matrix; `human_only`; the cancel-other-principal's-job rule as a policy hook used by `job.cancel_job@1`; refusal reasons.
**Out:** projection and pending requests (`CORE.BUS.POL.PROJ`); file handles (`CORE.BUS.POL.FS`); X1 approval (`CORE.BUS.POL.EGRESS`); motion release (`CMM`); the cancel command itself (`CORE.JOB.CMD`).

## Requirements

- **REQ-CORE.BUS.POL.CLASS-001** If an `agent` principal invokes an M1, S1, W3 or A0 command, then the bus shall return `policy` with reason `class_forbidden` and commit nothing.
- **REQ-CORE.BUS.POL.CLASS-002** If an `agent` or `script` principal invokes a command whose definition has `human_only: true`, then the bus shall return `policy` with reason `human_only` and commit nothing, whatever the command's class.
- **REQ-CORE.BUS.POL.CLASS-003** If a `script`, `service`, `device` or `system` principal invokes an M1, S1, W3 or A0 command, then the bus shall return `policy` with reason `class_forbidden`.
- **REQ-CORE.BUS.POL.CLASS-004** If `job.cancel_job@1` targets a job whose principal differs from the caller and the caller is not `human`, then the bus shall return `policy` with reason `not_job_owner`.
- **REQ-CORE.BUS.POL.CLASS-005** The matrix shall be a total function over every combination of principal kind, side-effect class and `human_only` value, published as `policy-matrix.json` for documentation generation.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pol/class/matrix.csv` (6 principal kinds x 11 classes x 2 `human_only` values = 132 rows with expected allow or reason), when `authorise_class` runs for each row, then every outcome matches.
- **AC-2** (error path) Given an agent calling a W3 test command, then `policy` / `class_forbidden` and nothing is journaled.
- **AC-3** (error path) Given a script calling a W2 test command with `human_only: true`, then `policy` / `human_only`; given a CLI session upgraded to human (D-012), then the same call is allowed.
- **AC-4** (error path) Given job J of script principal S, when agent A cancels J, then `policy` / `not_job_owner`; when S or a human cancels J, then allowed.

## Design

- **Registry command(s):** none.
- **Data structures:** `fn authorise_class(principal: &Principal, def: &CommandDef) -> Result<(), BusError>`; `fn authorise_cancel(caller: &Principal, job_owner: &PrincipalId) -> Result<(), BusError>`.
- **Algorithm:** exhaustive `match` on `(PrincipalKind, SideEffectClass, human_only)` with no wildcard arm; M1, S1, W3 and A0 are allowed for `human` only; motion release rules are checked later by `CMM` and the S1 ceremony is `TRUST`'s.
- **Errors:** `policy` (reasons `class_forbidden`, `human_only`, `not_job_owner`).
- **Determinism:** D0.
- **Performance budget:** < 1 µs per call [unverified until measured].
- **Crate:** `fid-bus` (module `policy::class`).

## Test fixtures

`fixtures/core/bus/pol/class/matrix.csv`, written from CONSTITUTION §3 and §3.1; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `policy::class` with `authorise_class(principal, def)` and `authorise_cancel(caller, job_owner)`. Rules: only `human` may run M1, S1, W3 or A0 (`class_forbidden` for every other kind); agent and script may not run any `human_only` command (`human_only`); only the job's own principal or a human may cancel a job (`not_job_owner`). Use an exhaustive match with no wildcard arm and emit `policy-matrix.json` through `fid-canon`. Tests: AC-1..AC-4 with `fixtures/core/bus/pol/class/matrix.csv`. Constraints: CONSTITUTION §3 (side-effect classes, human-only commands D-031, principals D-012), §3.1, §8.

## Open questions

- CONSTITUTION §3 bans only agents from M1, S1, W3 and A0 explicitly. This matrix also refuses them for `script`, `service`, `device` and `system`, following D-012's context (a script cannot release directly; a person upgrades the session) and D-030 (releases on server-held records are made in a desktop app). Replay re-executes W3 entries as `system` through `CORE.RPL`, not through this check. Confirm with `ENT.IAM` and `CORE.RPL.EXEC`.
- Build: REQ-002 names `agent` and `script` for `human_only`; the build also refuses `service`, `device` and `system` with `human_only` (fail closed: a human-only command is one only a human may run). The class ban is checked first, so a non-human principal invoking a human-only W3 command gets `class_forbidden`. `authorise_cancel` takes the job's recorded owner as `Option<&PrincipalRef>` (kind and identity, so `alice` as agent does not own `alice`'s script job; no recorded owner is open to a human only) instead of `&PrincipalId`. The matrix is published at `schema/policy-matrix.json`. Confirm or relax.

## Children

_Leaf: none._
