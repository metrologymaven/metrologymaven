---
id: D-008
title: Motion authorization rules for agent-authored CMM programs
status: accepted
raised_by: CMM split
blocks: [CMM.EXEC]
decide_by: before CMM.EXEC is expanded
updated: 2026-11-17
---

# D-008: Motion authorization rules for agent-authored CMM programs

## Question

1. Must a human **other than the requester** release a program that an agent wrote, in whole or in part, before it can run on a machine ("released-by-other")?
2. May an agent file even an **inert** run request (one that only queues a ticket for the Approval Station) through MCP?

## Context

Design Baseline §8 left question 1 pending a founder and counsel decision. Without one, a one-person CMM department could not release its own agent-written programs. The CMM agent took the stricter reading of question 2: no run requests over MCP, only `run:hold` and read-only status.

## Options

| Option | For | Against |
|---|---|---|
| A. Released-by-other required for agent-authored programs and the Regulated profile. Same person allowed otherwise, as two distinct acts with mandatory simulation and a setup run. Agents may file inert run requests | Matches the baseline proposal; usable in small shops | Agents can queue work that a human may approve in a hurry |
| B. As A, but agents may **not** file run requests (the stricter reading) | Smallest agent attack surface | Agents can't prepare a run batch for an operator |
| C. Released-by-other always | Strongest | Unworkable in one-person departments |

## Recommendation

B for the first release. Revisit A once the Approval Station has shown its diff and agent-authored badge in real use.

## Decision

**Accepted (option B), 2026-11-16, founder.** Released-by-other is required for agent-authored programs and under the Regulated profile; otherwise the same person may release and approve as two distinct acts with mandatory simulation and a setup run. **Agents may not file run requests over MCP**; their only motion-related tools are `run:hold` and read-only status. Revisit option A after field use of the Approval Station. **Counsel review is still recommended** before the first live-machine release. Follow-up: CONSTITUTION §3 amended.

**Amended by D-037 (2026-11-17):** agents may file run requests, but only for a program a human approved once under gate H1, within its approved motion envelope and unchanged program, machine, fixture and probe tuple. Any change to these requires a new H1 approval. Per-cycle physical start, the reduced-speed placement check and the setup-run rules are unchanged. Released-by-other is satisfied by construction.

