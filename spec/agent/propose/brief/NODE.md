---
id: AGENT.PROPOSE.BRIEF
title: Proposal brief schema and attach command
level: 3
parent: AGENT.PROPOSE
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-AGENT.PROPOSE-001, REQ-AGENT.PROPOSE-002, REQ-AGENT.PROPOSE-003, REQ-AGENT.PROPOSE-008]
covers: []
depends_on: [CORE.JRN.BR, CORE.REG.DEF, AUTO.MCP.TAINT]
uses_artifacts: []
side_effect: W1
determinism: D0
cli: [fid agent attach-brief]
mcp: [agent_attach_brief]
journal: [agent.attach_brief@1]
updated: 2026-11-17
---

# AGENT.PROPOSE.BRIEF: Proposal brief schema and attach command

## Purpose

Defines the `ProposalBrief` type (the words an agent supplies with its proposal) and the W1 command that attaches a brief to an agent branch, bound to the branch head it describes. Everything the agent writes here is untrusted text; nothing in it is a value of record.

## Scope

**In:**
- The `ProposalBrief` schema, its field limits and untrusted typing
- `agent.attach_brief@1` (W1): validation, branch check, head binding, journaling
- Re-attaching a newer brief (the latest attach on a branch wins)

**Out** (and which node owns it instead):
- Computing affected characteristics and decision moves (`AGENT.PROPOSE.IMPACT`)
- Completeness, staleness and claim mismatch (`AGENT.PROPOSE.SHOW`)
- Branches and promotion requests (`CORE.JRN.BR`)
- Untrusted rendering on MCP (`AUTO.MCP.TAINT`)

## Requirements

- **REQ-AGENT.PROPOSE.BRIEF-001** `agent.attach_brief@1` shall be declared side-effect class W1 and shall append its entry only to an agent branch; if the target branch is not an agent branch, then it shall fail with `not_agent_branch` and commit nothing.
- **REQ-AGENT.PROPOSE.BRIEF-002** When a brief is attached, the command shall record in its entry the digest of the branch head immediately before the attach entry (`bound_head`).
- **REQ-AGENT.PROPOSE.BRIEF-003** The schema shall hold `goal` (1 to 2,000 characters), `rationale` (1 to 20 steps of 1 to 500 characters each), `assumptions` (0 to 20 items of at most 500 characters), `confidence` (`low`, `medium` or `high`), `confidence_reasons` (1 to 5 items of at most 500 characters), `claimed_characteristics` (0 to 500 stable entity IDs), `client` and `model` (each 1 to 200 characters), and `skills_used` (0 to 50 Skill or recipe IDs).
- **REQ-AGENT.PROPOSE.BRIEF-004** If any field is missing, over its limit, or `confidence` is not one of the three words, then the command shall fail with `invalid_brief` naming every offending field, and commit nothing.
- **REQ-AGENT.PROPOSE.BRIEF-005** Every free-text field of the brief (`goal`, `rationale`, `assumptions`, `confidence_reasons`, `client`, `model`) shall be declared untrusted in the registry schema, so that every surface returns it with the `AUTO.MCP.TAINT` marking.
- **REQ-AGENT.PROPOSE.BRIEF-006** The schema shall contain no numeric confidence field, and if the request carries any field not in the schema, then the command shall fail with `invalid_brief`.
- **REQ-AGENT.PROPOSE.BRIEF-007** When a branch already carries a brief, a new attach shall append a new entry, and the latest attach entry on the branch shall be the current brief.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/agent/propose/branch-basic` with agent branch `agent/s1` and fixture brief `fixtures/agent/propose/brief-valid.json`, when `fid agent attach-brief --branch agent/s1 --brief root:fixtures/brief-valid.json --json` runs as an agent principal, then the result holds the entry ID and `bound_head` equal to the head digest before the call, and the entry is on `agent/s1` only.
- **AC-2** (error path) Given branch `main` (not an agent branch), when the attach runs, then it fails with `not_agent_branch` and the journal is unchanged.
- **AC-3** (error path) Given `fixtures/agent/propose/brief-bad.json` with a 2,001-character goal, 21 rationale steps and `confidence: 0.8`, then the command fails with `invalid_brief` naming `goal`, `rationale` and `confidence`, and journals nothing.
- **AC-4** Given the brief from AC-1 read back over MCP (`agent_show_brief` stub or registry describe), then every free-text field appears as `{"untrusted": true, "text": ...}`.
- **AC-5** Given two attaches on `agent/s1`, then the second entry is reported as the current brief.
- **AC-6** (replay) Given the journal from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `agent.attach_brief@1`, W1, `fs_effect: none`. Parameters: `branch: BranchRef`, `brief: ProposalBrief` (inline object; the CLI accepts a read-root handle to a JSON file and inlines it). Result: `{ entry_id, bound_head: Digest }`.
- **Data structures:** `ProposalBrief { goal, rationale: Vec<String>, assumptions: Vec<String>, confidence: Confidence { Low | Medium | High }, confidence_reasons: Vec<String>, claimed_characteristics: Vec<EntityId>, client, model, skills_used: Vec<SkillRef> }`. Character counts are Unicode scalar values. Free-text fields carry the registry `untrusted` schema keyword (`CORE.REG.DEF`).
- **Algorithm:** resolve the branch; check it is an agent branch (`CORE.JRN.BR` branch kind); validate the whole brief and collect all errors; read the head digest; append the entry with the brief and `bound_head`.
- **Errors:** `not_agent_branch`, `unknown_branch`, `invalid_brief { fields }`.
- **Determinism:** D0; no numeric computation, canonical JSON payload.
- **Performance budget:** < 5 ms per attach excluding journal fsync [unverified until measured].
- **Crate:** `fid-agent`

## Test fixtures

Project-authored: `fixtures/agent/propose/branch-basic` (a small project with one agent branch of 3 commands), `brief-valid.json`, `brief-bad.json`. No third-party data.

## Build prompt

> In crate `fid-agent` (layer L3), implement the registry command `agent.attach_brief@1` (side-effect class W1; CLI `fid agent attach-brief`, MCP tool `agent_attach_brief`). Define `ProposalBrief` with fields and limits: goal 1-2,000 chars; rationale 1-20 steps of 1-500 chars; assumptions 0-20 items of at most 500 chars; confidence one of `low|medium|high` (a word, never a number; reject unknown fields); confidence_reasons 1-5 items of at most 500 chars; claimed_characteristics 0-500 stable entity IDs; client and model 1-200 chars; skills_used 0-50 IDs. Declare every free-text field `untrusted` in the registry schema. The command must refuse any branch that is not an agent branch (`not_agent_branch`), validate all fields and report every failure at once (`invalid_brief`), and append one entry recording the brief and `bound_head`, the branch head digest just before the append. The latest attach on a branch is the current brief. Acceptance: AC-1 to AC-6 of AGENT.PROPOSE.BRIEF with fixtures under `fixtures/agent/propose/`. Constraints: CONSTITUTION §8 (no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`, bounded parsing); C5/C6 (agent text is untrusted data); agents never pass raw paths, so the CLI accepts only a root handle for the brief file.

## Open questions

- REQ-CORE.JRN.BR-007 accepts a promotion request without a brief. Whether CORE should refuse an agent promotion request with no complete brief is for CORE to decide; this node only provides the data.

## Children

_Leaf: none._
