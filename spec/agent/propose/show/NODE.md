---
id: AGENT.PROPOSE.SHOW
title: Brief completeness, staleness and claim mismatch
level: 3
parent: AGENT.PROPOSE
kind: feature
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AGENT.PROPOSE-003, REQ-AGENT.PROPOSE-005, REQ-AGENT.PROPOSE-006, REQ-AGENT.PROPOSE-007]
covers: []
depends_on: [AGENT.PROPOSE.BRIEF, AGENT.PROPOSE.IMPACT, AUTO.MCP.TAINT]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: [fid agent show-brief]
mcp: [agent_show_brief]
journal: [agent.show_brief@1]
updated: 2026-11-17
---

# AGENT.PROPOSE.SHOW: Brief completeness, staleness and claim mismatch

## Purpose

Returns everything a human (or the eval harness) needs to judge a proposal without reading the raw diff: the current brief, the computed impact, and one verdict saying whether the proposal is `complete`, `stale`, missing its brief or missing fields, plus every mismatch between what the agent claims and what the diff shows.

## Scope

**In:**
- `agent.show_brief@1` (R0) and the `ProposalVerdict` type
- Staleness rule, completeness rule, claim-mismatch rule

**Out** (and which node owns it instead):
- Brief schema and attach (`AGENT.PROPOSE.BRIEF`)
- Impact computation (`AGENT.PROPOSE.IMPACT`)
- Rendering in the Proposal Inbox (`VIZ.INBOX`)

## Requirements

- **REQ-AGENT.PROPOSE.SHOW-001** `agent.show_brief@1` shall be class R0 and shall return the current brief, the computed impact for the branch's current head, the verdict and the claim mismatches.
- **REQ-AGENT.PROPOSE.SHOW-002** The brief shall be reported `stale` when any entry other than an `agent.attach_brief@1` entry follows `bound_head` on the branch.
- **REQ-AGENT.PROPOSE.SHOW-003** The verdict shall be exactly one of `complete`, `brief_missing`, `stale` or `fields_missing` (with the field names), and shall be `complete` only when the current brief exists, is not stale and passes the schema of `AGENT.PROPOSE.BRIEF`.
- **REQ-AGENT.PROPOSE.SHOW-004** The result shall list `claim_mismatch` items for each characteristic in the brief's `claimed_characteristics` that the impact omits (`claimed_not_changed`) and each characteristic in the impact that the brief omits (`changed_not_claimed`).
- **REQ-AGENT.PROPOSE.SHOW-005** Every agent-authored string in the result shall be returned with the untrusted marking, and every system-computed field shall not be.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/agent/propose/show-complete` (brief attached at the head, claims equal to the impact), when `fid agent show-brief --branch agent/s1 --json` runs, then the verdict is `complete` with no mismatches.
- **AC-2** Given `show-stale` (one W1 command after the brief), then the verdict is `stale`.
- **AC-3** Given `show-missing` (no brief), then the verdict is `brief_missing`.
- **AC-4** Given `show-mismatch` (brief claims C3; impact lists C1 only), then mismatches are `claimed_not_changed: C3` and `changed_not_claimed: C1`, and the verdict is still `complete`.
- **AC-5** Given AC-1 over MCP, then `goal` and `rationale` items are `{"untrusted": true, ...}` and `verdict` is a plain enum.
- **AC-6** Given AC-1, then nothing is written to the Record Journal (R0, D-031).

## Design

- **Registry command(s):** `agent.show_brief@1`, R0. Parameters: `branch: BranchRef`. Result: `{ brief: Option<ProposalBrief>, impact: ProposalImpact, verdict: ProposalVerdict, mismatches: Vec<ClaimMismatch> }`.
- **Data structures:** `ProposalVerdict { Complete | BriefMissing | Stale | FieldsMissing(Vec<FieldName>) }`; `ClaimMismatch { id, kind: ClaimedNotChanged | ChangedNotClaimed }`.
- **Algorithm:** find the latest attach entry; check entries after `bound_head`; revalidate the stored brief against the current schema; call the impact function in-process; compare ID sets.
- **Errors:** `unknown_branch`.
- **Determinism:** D0.
- **Performance budget:** dominated by `agent.compute_impact@1`.
- **Crate:** `fid-agent`

## Test fixtures

Project-authored synthetic projects `show-complete`, `show-stale`, `show-missing`, `show-mismatch` under `fixtures/agent/propose/`.

## Build prompt

> In crate `fid-agent` (layer L3), implement `agent.show_brief@1` (class R0; CLI `fid agent show-brief`, MCP `agent_show_brief`). For an agent branch, return the current brief (latest `agent.attach_brief@1` entry), the computed `ProposalImpact` (reuse the in-crate impact function), a verdict (`complete` only if the brief exists, no entry other than an attach entry follows its `bound_head`, and it passes the brief schema; else `brief_missing`, `stale`, or `fields_missing` with names), and claim mismatches (`claimed_not_changed`, `changed_not_claimed`) between the brief's claimed characteristics and the impact. Return agent-authored strings with the untrusted marking and system fields without it. Acceptance: AC-1 to AC-6 of AGENT.PROPOSE.SHOW. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.); R0 reads go to the Activity Log, not the Record Journal (D-031); C6.

## Open questions

- The verdict is the completeness contract consumed by `VIZ.INBOX` and by `AGENT.HARNESS.SCORE` (via `fid agent show-brief --json`).

## Children

_Leaf: none._
