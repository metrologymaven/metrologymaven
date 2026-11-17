---
id: D-031
title: Constitution gaps found at level 3 (journaling of reads, human-only commands, missing classes)
status: proposed
raised_by: level-3 review
blocks: []
decide_by: before level-3 expansion
updated: 2026-11-16
---

# D-031: Constitution gaps found at level 3

## Question

Several agents had to guess the same rules. Settle them once, in CONSTITUTION §3 and §6.

## Proposals

| # | Gap (raised by) | Proposal |
|---|---|---|
| 1 | **Are R0/R1 commands journaled?** (SCAN, PLAT, CORE, VIZ) | R0 and R1 are **not** written to the Record Journal. They go to the unchained Activity Log, and only while a recording session is active or when the command is a step of a Command Program, so programs and tutorials can replay reads. Exception: a W-class command that consumes an R1 result journals that result's digest. |
| 2 | **No generic "human only" mechanism** (AUTO.REC.ADOPT, ALN.MODEL.ADOPT, GDT.PMI, RPT.BALLOON.ENTRY) | Add a registry field `human_only: true`. The bus refuses agent and script principals for such commands whatever their class, and MCP projects them as `request_only`. Adoption commands (`*.adopt_*`) and manual readings use it. Keep them W2 unless they release a controlled record (W3). |
| 3 | Side-effect class of the **motion hold request** (CMM.EXEC.REQ) | A new note in §3: the hold request is **W1**, rate-limited, journaled and advisory. It is the only MCP-originated motion effect, as the baseline already states. |
| 4 | Class of **launching another app** (ECO.APPS.HANDOFF) | Not a command. It is a UI or OS action. Passing a project between apps is just opening it. |
| 5 | Class of **changing update-check or telemetry consent** (ECO.REL.UPD, CORE.STO.PRIV) | **A0** for site-wide policy (administrator). **W2** for a user's own consent, journaled. Consent never defaults to on. |
| 6 | **Generic egress command** (VIZ.CAPTURE, PLAT.DOCTOR.FEEDBK, export to network shares) | CORE.BUS owns one X1 primitive, `egress.send@1`. It takes a staged artifact and a declared destination, and is request-only for agents. Other domains stage artifacts and call it. |
| 7 | **Single-class leaves** (IO formats with import + export) | Already fixed in tooling: `side_effect:` may be a list, one class per command. |

## Recommendation

Accept all seven and amend CONSTITUTION §3 and §6.

## Decision

_Open._
