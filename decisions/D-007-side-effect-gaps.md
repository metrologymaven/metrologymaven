---
id: D-007
title: Side-effect classes the constitution does not yet assign
status: accepted
raised_by: level check L2
blocks: [IO.REG, RPT.OUTPUT, ECO.SDK, IND.CONTRACT, CORE.JOB, LVM.GUIDE, AUTO.MCP]
decide_by: before the first export or plugin leaf
updated: 2026-11-16
---

# D-007: Side-effect classes the constitution does not yet assign

## Question

Which side-effect class (CONSTITUTION §3) applies to each of these operations, which several level-2 nodes needed and assumed differently?

## Context and proposal

| Operation | Raised by | Proposal | Why |
|---|---|---|---|
| Write an export file into an approved **local** export root | IO.REG, RPT.OUTPUT | **W1** with `fs_effect: write_root`; X1 only when the destination leaves the machine (network share, upload) | Data that stays local does not leave the tenant; agents still need confirmation for file writes (baseline §3.2) |
| Install, enable, update or remove a plugin or industry pack | ECO.SDK, IND.CONTRACT | **A0** (never on MCP); listing is R0 | Adds executable code to the engine |
| Cancel a job | CORE.JOB | **W1** for the requester's own jobs; another principal's job needs a human | Cancelling changes outcomes but never records |
| Sound or visual alarms during live guidance | LVM.GUIDE | **R0** UI effect, not a command class; acknowledging an alarm is **W2** (journaled) | The acknowledgement is the record |
| MCP "describe, then invoke by command ID" dispatcher for progressive disclosure | AUTO.MCP | Allowed. It is generated, not a hand-written alias, and is limited to R0/R1 | Matches baseline §3.2 (`command_invoke` reaches R0/R1 only) |
| CLI verbs outside `fid <domain> <verb-object>` (e.g. `fid doctor`) | PLAT.DOCTOR | Allow a single-verb command when the domain *is* the verb, generated from the registry | Keeps CLI ergonomics without aliases |

## Recommendation

Accept the proposals as written and amend CONSTITUTION §3 with a short table.

## Decision

**Accepted as proposed, 2026-11-16, founder.** The proposals table above becomes CONSTITUTION §3.1.
