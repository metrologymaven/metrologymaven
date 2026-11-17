---
id: D-035
title: Constitution gaps found at level 4 (injected input, egress responses, foundation crates)
status: proposed
raised_by: launch-1 review
blocks: []
decide_by: before the next expansion
updated: 2026-11-17
---

# D-035: Constitution gaps found at level 4

## Question

The 33 launch-1 batches independently ran into the same handful of gaps. Settle them in CONSTITUTION §2.1, §3 and §3.1.

## Proposals

| # | Gap (raised by) | Proposal |
|---|---|---|
| 1 | **Security: injected UI input runs as `human`** (AUTO.CTRL, VIZ.SHELL.A11Y.UIHOST, CORE.BUS.PRIN) | A GUI action is `human` only when its input comes from local OS input devices. Input injected through the control channel, accessibility automation or any agent surface carries **that channel's principal** (agent or script), so it cannot press a `human_only` or W3 button. The bus takes an origin token from the UI host. |
| 2 | **Registry queries list hidden commands** (AUTO.MCP) | `registry.list_commands@1` and `describe_command@1` filter by the caller's projection, so MCP callers never see `never`-projected commands (M1, S1, A0). |
| 3 | **`egress.send@1` is send-only** (UNC.RDL.TRACIM, ECO.REL.UPD, TRUST.REPLAY.KART) | `egress.send@1` returns a bounded, size-capped response body. Pure fetches use the same X1 primitive with an empty payload. Responses are untrusted data (C6). |
| 4 | **Foundation layer (L0) is missing pure crates** (ALN, IO, GDT, RPT, SCAN, UNC, AGENT) | Add to L0: the GD&T evaluation library `fid-gps` (GDT pure math); the **ResultRecord schema crate** (types only; GDT.CHAR.RECORD keeps the assembly logic); exact geometric predicates as a new **KRN.NUM.PRED** child (no FMA); the I18N locale render transform; and the UNC instrument-model kernel (`fid-kunc`). Tooling may link pure L0 libraries (for example canonical JSON) instead of re-implementing them. |
| 5 | **Layer of a few crates** (GEO.OOC, QUAL, SCAN preview, RPT designer UI) | GEO out-of-core storage is L2 (it uses CORE.STO). The QUAL product crate, the SCAN GPU-preview crate and the RPT designer UI are L3. |
| 6 | **Lean protected-branch promotion has no human assertion in launch-1** (VIZ.INBOX, CORE.JRN.BR.PROMO, QUAL.IQOQ) | In the Lean profile, the human assertion for promotion and W3 release is CORE.BUS.PRIN's `HumanPresence` confirmation (OS user verification, as in baseline P1a). TRUST.ESIG remains the Regulated path. Signing stays optional in Lean (baseline P9). |
| 7 | **Consistency fixes** | CORE.STO.PRIV's telemetry consent becomes W2 (§3.1 already says so). The R0 host handshake under D-023 gets a scoped, read-only socket permit in CORE.BUS.POL.EGRESS. |

## Recommendation

Accept all seven. #1 is the most important: without it, an agent driving the UI could act as a human.

## Decision

_Open._
