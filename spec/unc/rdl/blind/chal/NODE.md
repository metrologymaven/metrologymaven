---
id: UNC.RDL.BLIND.CHAL
title: The rdl.submit_challenge@1 command
level: 4
parent: UNC.RDL.BLIND
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.BLIND-003]
covers: []
depends_on: [UNC.RDL.FORMAT, KRN.MAN.KCI, CORE.BUS.PIPE, CORE.REG.DEF]
side_effect: R1
determinism: D1
cli: [fid rdl submit-challenge]
mcp: [rdl_submit_challenge]
journal: [rdl.submit_challenge@1]
updated: 2026-11-17
---

# UNC.RDL.BLIND.CHAL: The rdl.submit_challenge@1 command

## Purpose

Lets any verifier test the kernel on their own data without revealing their answer. The challenger submits points and an aim and receives the kernel's result with the kernel identity. The command refuses any submission that contains a reference value, so the kernel side never sees the challenger's reference.

## Scope

**In:**
- `rdl.submit_challenge@1` (R1): params `aim`, `points` (import handle or inline, unit m or mm), optional `params`; returns the kernel result, algorithm ID, kernel semver and manifest digest
- Schema `rdl-challenge/v1` with no field for references; unknown fields rejected

**Out:**
- Comparing with the challenger's reference (done by the challenger)
- Hosted service (ENT)

## Requirements

- **REQ-UNC.RDL.BLIND.CHAL-001** When `rdl.submit_challenge@1` receives points and a known aim, the system shall return the kernel result with algorithm ID, kernel semver and manifest digest.
- **REQ-UNC.RDL.BLIND.CHAL-002** If the submission contains any field outside `rdl-challenge/v1`, including any reference or expected-value field, then the command shall reject it with `reference_not_accepted` or `unknown_field` and compute nothing.
- **REQ-UNC.RDL.BLIND.CHAL-003** If the submission exceeds 10^6 points or 64 MiB, then the command shall fail with `limit_exceeded`.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/blind/chal/circle_pts.json` and aim `fit.circle.ls`, when `fid rdl submit-challenge --aim fit.circle.ls --points root://in/circle_pts.json --json` runs, then the kernel result is returned with semver and manifest digest.
- **AC-2** (error path) Given `circle_with_ref.json` (adds `expected_radius`), then `reference_not_accepted` and the kernel is not called.
- **AC-3** (error path) Given an unknown aim, then `unknown_aim`.
- **AC-4** Given AC-1, then no Record Journal entry is written.

## Design

- **Registry command(s):** `rdl.submit_challenge@1` (R1; returns `ChallengeResult`).
- **Data structures:** `rdl-challenge/v1` (aim, unit, points, params); `ChallengeResult`.
- **Algorithm:** strict bounded parse; convert to SI; kernel `evaluate`; return.
- **Errors:** `reference_not_accepted`, `unknown_field`, `unknown_aim`, `limit_exceeded`, kernel errors.
- **Determinism:** D1.
- **Performance budget:** parse overhead under 10 % of fit time [unverified].
- **Crate:** `fid-rdl` (module `challenge`).

## Test fixtures

`fixtures/rdl/blind/chal/` synthetic submissions (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl`, module `challenge`, add registry command `rdl.submit_challenge@1` (R1; CLI `fid rdl submit-challenge`; MCP `rdl_submit_challenge`; `--json`). Parse `rdl-challenge/v1` strictly (aim, unit m or mm, points, optional params; any other field gives `unknown_field`, and fields named like references or expected values give `reference_not_accepted`), with bounds (10^6 points, 64 MiB). Convert to SI, call the kernel `evaluate` with the aim, and return the result with algorithm ID, kernel semver and manifest digest. No Record Journal entry. Tests AC-1..AC-4. CONSTITUTION 8; handles only, no raw paths for agents.

## Open questions

- None.

## Children

_Leaf: none._
