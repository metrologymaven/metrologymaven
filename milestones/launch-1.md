---
id: launch-1
title: Inspect + Report beachhead (free scan inspection, reporting and open-oracle verification)
status: proposed
targets: [CORE, KRN, GEO, IO, ALN, GDT, UNC, SCAN, VIZ, RPT, TRUST, AUTO, PLAT, ECO, QUAL, MIG, FLOW, UX, OUTR, AGENT, I18N, HAL.IDL, HAL.ACQ]
exclude: [CMM, LVM, IND, ENT, I18N.XLATE, I18N.GLOSS, I18N.SCRIPT, I18N.CONTENT, HAL.IPP, HAL.LIVE, HAL.DRV, HAL.PAIR, HAL.HOST, HAL.SIM, HAL.XSCR, HAL.ACQ.INGEST.BENCH, AUTO.PY, AUTO.SCRIPTED, TRUST.ESIG, UNC.PLAN, UNC.XFM, UNC.RDL.BLIND, UNC.RDL.EXT.NIST, UNC.RDL.EXT.PTBSG, UNC.RDL.EXT.FILT, UNC.RDL.TRACIM, VIZ.WEB, VIZ.INBOX.GATES.REG, VIZ.SHELL.TREE.CONSOLE, ECO.SDK, ECO.APPS.PROBE, ECO.APPS.NETWORK, ECO.COMM.PARTNER, PLAT.XHOST, PLAT.WEB, OUTR.PROG, OUTR.COMM, OUTR.GDTC, OUTR.BENCH.CASE, OUTR.BENCH.CMP, OUTR.LAUNCH.ANN, OUTR.LAUNCH.KIT, OUTR.LAUNCH.REVIEW, OUTR.CONTENT.VIDEO, OUTR.CONTENT.MIG.PCD, OUTR.CONTENT.MIG.SA, OUTR.CONTENT.TUT.PROBE, OUTR.CONTENT.TUT.NETWORK, OUTR.DOCS.BOOK.PROBE, OUTR.DOCS.BOOK.NETWORK, UX.SCREENS.PROBE, UX.SCREENS.NETWORK, AGENT.HARNESS.PANEL, MIG.PROGEQ, FLOW.PROBE, FLOW.NETWORK, FLOW.INSPECT.J7GF]
updated: 2026-11-22
---

# Milestone launch-1: Inspect + Report beachhead

## Goal

Ship the first two focused apps, **Metrology Maven Inspect** (scan-to-CAD inspection) and **Metrology Maven Report** (transcription and inspection reports, basic SPC), plus the `fid` CLI, MCP servers, and `fid-verify`. Back them with a **published open-oracle verification study** over project-authored mathematical generators and reference suites (`D-038`, `D-052`).

This category is software-only (no physical machine motion), open-standards-first, and self-contained with zero external outreach blockers.

## Exit criteria

- These FLOW acceptance jobs pass their gating rules:
  - JOB.ACC.01: scan FAI from CAD + PMI
  - JOB.ACC.02: transcription AS9102
  - JOB.ACC.03: drawing-only FAI
  - JOB.ACC.05: kiosk run
  - JOB.ACC.07: **sections and thickness only**. Gap and flush needs the IND.GAPFL pack (P3) and moves to launch-2.
- Verification study #1 is published under the MIG / UNC.RDL protocol (D-021 Option A, D-038, D-052): results are verified against project-authored clean-room mathematical generators and arbitrary-precision oracles, with zero named-incumbent comparisons and zero external data-owner outreach. Unreviewed GD&T rules carry `interpretation: provisional` (`D-045`, `D-052`).
- PLAT Tier 1 platforms are green in CI, and the release ships through ECO.REL with QUAL's SBOM and IQ/OQ pack.
- AGENT's evaluation suite runs automatically against the shipped MCP servers using deterministic scripted adversaries and `AGENT.LOCAL`, and the scoreboard is published.

## Out of scope

- CMM programming and execution (Probe app, P2)
- Large-volume networks (Network app, P3)
- Industry packs
- Enterprise server
- Live instruments beyond file import and manual gauge entry
- Python scripting
- Regulated e-signatures
- Web build
- Languages other than English
- External expert/partner/legal outreach, human review panels, surveys, blind custodians, and external NMI data redistribution (`D-052`)

`scope.py` reports any in-scope node that depends on an excluded one, so the exclusions stay honest.

## How to use

```bash
python tools/scope.py milestone milestones/launch-1.md
```

This writes `generated/milestones/launch-1.md`:
- the dependency closure;
- the leaves in **build waves** (each wave can be built in parallel);
- the open nodes that still need expanding;
- the decisions blocking the milestone.
