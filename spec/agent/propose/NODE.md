---
id: AGENT.PROPOSE
title: Proposal brief and computed impact
level: 2
parent: AGENT
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-006]
covers: []
depends_on: [CORE.JRN.BR, AUTO.MCP.TAINT]
cli: [fid agent attach-brief, fid agent compute-impact, fid agent show-brief]
mcp: [agent_attach_brief, agent_compute_impact, agent_show_brief]
journal: [agent.attach_brief@1]
updated: 2026-11-17
---

# AGENT.PROPOSE: Proposal brief and computed impact

## Purpose

Defines what an agent proposal contains so a human can adopt or reject it from the Proposal Inbox without reading the raw diff. The agent supplies only words (goal, rationale, assumptions, confidence); the system computes the affected characteristics and the predicted impact from the branch diff, so an agent cannot misstate what its work changes. It is separate from VIZ.INBOX because the Inbox renders proposals while this node defines and produces their content, and the same content is scored by the eval harness.

## Scope

**In:**
- The `ProposalBrief` schema: agent-authored fields, all untrusted text
- `agent.attach_brief@1`: attach a brief to an agent branch head
- `agent.compute_impact@1`: affected characteristics and predicted impact from the branch diff
- `agent.show_brief@1`: brief, computed impact, completeness, staleness and claim mismatches
- The completeness rule used by the Inbox and by the eval harness

**Out** (and which node owns it instead):
- Branches, diff and promotion requests (`CORE.JRN.BR`)
- Rendering the brief and the four-layer diff (`VIZ.INBOX`)
- Untrusted marking at the MCP boundary (`AUTO.MCP.TAINT`)
- Applying or rejecting a proposal (human only, `CORE.JRN.BR` and `VIZ.INBOX`)

## Requirements

- **REQ-AGENT.PROPOSE-001** `agent.attach_brief@1` shall be class W1, shall commit only to an agent branch, and shall bind the brief to the branch head digest at the time it is attached.
- **REQ-AGENT.PROPOSE-002** A brief shall hold a goal (at most 2,000 characters), at most 20 rationale steps of at most 500 characters each, a list of assumptions, a confidence level word (`low`, `medium` or `high`) with at least one reason, the declared client and model, and the IDs of the Skills and recipes used.
- **REQ-AGENT.PROPOSE-003** Every agent-authored brief field shall be typed untrusted and shall be returned with the untrusted marking of `AUTO.MCP.TAINT`.
- **REQ-AGENT.PROPOSE-004** `agent.compute_impact@1` shall be class R1 and shall derive, from the branch diff against its target only, the list of characteristics whose value, decision outcome or uncertainty class would change, the count of decisions moving between each pair of outcomes, the command count and the set of side-effect classes on the branch.
- **REQ-AGENT.PROPOSE-005** If the brief names a characteristic that the computed impact does not list, or the computed impact lists one the brief omits, then `agent.show_brief@1` shall report `claim_mismatch` with each difference.
- **REQ-AGENT.PROPOSE-006** When the branch head no longer matches the digest bound to the brief, `agent.show_brief@1` shall report the brief as `stale`.
- **REQ-AGENT.PROPOSE-007** `agent.show_brief@1` shall report a proposal as `complete` only when a non-stale brief with every field of REQ-AGENT.PROPOSE-002 exists and the computed impact is current; otherwise it shall report `brief_missing`, `stale` or the names of the missing fields.
- **REQ-AGENT.PROPOSE-008** The confidence level shall be stored as a word only, and the brief schema shall have no numeric confidence field.

## Interfaces

- **Offers:** `ProposalBrief` and `ProposalImpact` types; `agent.attach_brief@1` (W1), `agent.compute_impact@1` (R1), `agent.show_brief@1` (R0); the completeness verdict used by `VIZ.INBOX` and `AGENT.HARNESS`.
- **Consumes:** `CORE.JRN.BR` branch diff and promotion-request records; ResultRecord decisions and uncertainty classes through the diff's output digests; `AUTO.MCP.TAINT` untrusted marking.

## Decomposition hints

SCHEMA (brief type and limits), ATTACH (W1 command and head binding), IMPACT (diff to affected characteristics and decision moves), SHOW (completeness, staleness and claim mismatch).

## Split rationale

Split by **who produces the data**: the agent's words (BRIEF, W1), the system's computation from the diff (IMPACT, R1), and the verdict that compares the two (SHOW, R0). Each child is one registry command in crate `fid-agent`. Alternative considered: a separate SCHEMA child; rejected because the schema has exactly one writer (the attach command) and would be a leaf with no command of its own.

## Open questions

- REQ-CORE.JRN.BR-007 accepts a promotion request without a brief. Should CORE refuse an agent promotion request whose source head has no complete brief, or should the Inbox only flag it? Needs CORE agreement; AGENT cannot change CORE.JRN.BR.
- VIZ.INBOX should consume this node's types for its rationale and confidence rendering (REQ-VIZ.INBOX-006 and -007); VIZ.INBOX.* may depend on `AGENT.PROPOSE` without creating a cycle, because this node depends only on CORE and AUTO nodes.
- Predicted impact on reports and SPC (beyond characteristics) is left to a later revision.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AGENT.PROPOSE.BRIEF | Proposal brief schema and attach command | 001 002 003 008 | none |
| AGENT.PROPOSE.IMPACT | Computed proposal impact from the branch diff | 004 | none |
| AGENT.PROPOSE.SHOW | Brief completeness, staleness and claim mismatch | 003 005 006 007 | none |
