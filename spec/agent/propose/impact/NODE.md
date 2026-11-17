---
id: AGENT.PROPOSE.IMPACT
title: Computed proposal impact from the branch diff
level: 3
parent: AGENT.PROPOSE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.PROPOSE-004]
covers: []
depends_on: [CORE.JRN.BR, CORE.STO]
uses_artifacts: []
side_effect: R1
determinism: D0
cli: [fid agent compute-impact]
mcp: [agent_compute_impact]
journal: [agent.compute_impact@1]
updated: 2026-11-17
---

# AGENT.PROPOSE.IMPACT: Computed proposal impact from the branch diff

## Purpose

Computes, from the branch diff alone, what an agent's proposal would change: which characteristics change value, decision outcome or uncertainty class, how many decisions move between outcomes, how many commands the branch holds and which side-effect classes they use. The agent cannot influence this result except through the commands it actually ran.

## Scope

**In:**
- `agent.compute_impact@1` (R1) and the `ProposalImpact` type
- Mapping changed entities in the diff to ResultRecords and comparing old and new decision outcome, value and UEC class

**Out** (and which node owns it instead):
- The branch diff itself (`CORE.JRN.BR`, `journal.diff_branch@1`)
- Comparing the impact with the brief's claims (`AGENT.PROPOSE.SHOW`)
- Impact on reports and SPC (not in this revision)

## Requirements

- **REQ-AGENT.PROPOSE.IMPACT-001** `agent.compute_impact@1` shall be class R1, shall read only the branch diff against the branch's promotion target and the ResultRecords whose digests the diff names, and shall change no persistent state.
- **REQ-AGENT.PROPOSE.IMPACT-002** The result shall list every characteristic whose ResultRecord is added, removed, or changed in value, decision outcome or uncertainty class, each with old and new outcome and UEC class, sorted by stable entity ID.
- **REQ-AGENT.PROPOSE.IMPACT-003** The result shall give a 4 by 4 count matrix of decision moves over the outcomes `CONFORMS`, `NONCONFORMS`, `INDETERMINATE` and `NOT_DECIDABLE`, plus counts of added and removed decisions.
- **REQ-AGENT.PROPOSE.IMPACT-004** The result shall give the number of journal entries on the branch since its fork point and the sorted set of side-effect classes those entries declare.
- **REQ-AGENT.PROPOSE.IMPACT-005** The result shall carry the source head digest and target head digest it was computed from.
- **REQ-AGENT.PROPOSE.IMPACT-006** The result shall report values only as the old and new values already stored in the ResultRecords, and shall compute no new value.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/agent/propose/impact-two-moves` (agent branch that refits one feature, turning characteristic C1 from `CONFORMS` to `NONCONFORMS` and changing C2's value only), when `fid agent compute-impact --branch agent/s1 --json` runs, then the result lists C1 and C2 in ID order, the move matrix has 1 in (CONFORMS, NONCONFORMS) and 0 elsewhere, and the class set equals the golden file.
- **AC-2** Given a branch with no commands since its fork point, then the result has no characteristics, an all-zero matrix and entry count 0.
- **AC-3** (error path) Given an unknown branch, then the command fails with `unknown_branch` and writes nothing to the Record Journal.
- **AC-4** Given AC-1 run twice, then both results are byte-identical in canonical JSON.

## Design

- **Registry command(s):** `agent.compute_impact@1`, R1. Parameters: `branch: BranchRef`. Result: `ProposalImpact`.
- **Data structures:** `ProposalImpact { source_head, target_head, characteristics: Vec<ImpactRow { id, change: Added|Removed|Changed, old_outcome, new_outcome, old_uec, new_uec, value_changed: bool }>, moves: [[u32; 4]; 4], added: u32, removed: u32, entry_count: u32, classes: BTreeSet<SideEffect> }`.
- **Algorithm:** call the diff (`CORE.JRN.BR`); for each changed entity of kind ResultRecord, load old and new records by digest from the store (`CORE.STO`); compare outcome, UEC class and value bits; tally; collect classes from the branch entries' command definitions. `value_changed` compares stored f64 bits, never recomputes.
- **Errors:** `unknown_branch`, `no_target` (branch has no promotion target), `record_missing { digest }`.
- **Determinism:** D0; integer counting and ordered collections only.
- **Performance budget:** < 200 ms for a branch changing 5,000 characteristics [unverified until measured].
- **Crate:** `fid-agent`

## Test fixtures

Project-authored synthetic projects under `fixtures/agent/propose/`: `impact-two-moves`, `impact-empty`, with golden results.

## Build prompt

> In crate `fid-agent` (layer L3), implement `agent.compute_impact@1` (class R1; CLI `fid agent compute-impact`, MCP `agent_compute_impact`). Input: an agent branch. Call the CORE branch diff against the branch's promotion target, load old and new ResultRecords by digest, and return `ProposalImpact`: changed characteristics sorted by stable ID with old/new decision outcome and UEC class and a `value_changed` flag (stored-bits comparison, never recomputation), a 4x4 decision-move matrix over CONFORMS/NONCONFORMS/INDETERMINATE/NOT_DECIDABLE plus added/removed counts, the entry count since the fork point, the sorted set of side-effect classes, and the source and target head digests. Change no persistent state. Errors: `unknown_branch`, `no_target`, `record_missing`. Acceptance: AC-1 to AC-4 of AGENT.PROPOSE.IMPACT. Constraints: CONSTITUTION §8 (no panic, deny `unwrap` etc.), C1 (compute no value of record; report stored values only), no std HashMap/HashSet in result ordering.

## Open questions

- Predicted impact on reports and SPC is deferred to a later revision.

## Children

_Leaf: none._
