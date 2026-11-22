---
id: launch-1
title: Inspect + Report beachhead (free scan inspection, reporting and published correlation)
status: proposed
targets: [CORE, KRN, GEO, IO, ALN, GDT, UNC, SCAN, VIZ, RPT, TRUST, AUTO, PLAT, ECO, QUAL, MIG, FLOW, UX, OUTR, AGENT, I18N, HAL.IDL, HAL.ACQ]
exclude: [CMM, LVM, IND, ENT, I18N.XLATE, I18N.GLOSS, I18N.SCRIPT, I18N.CONTENT, HAL.IPP, HAL.LIVE, HAL.DRV, HAL.PAIR, HAL.HOST, HAL.SIM, HAL.XSCR, HAL.ACQ.INGEST.BENCH, AUTO.PY, AUTO.SCRIPTED, TRUST.ESIG, UNC.PLAN, UNC.XFM, VIZ.WEB, VIZ.INBOX.GATES.REG, VIZ.SHELL.TREE.CONSOLE, ECO.SDK, ECO.APPS.PROBE, ECO.APPS.NETWORK, PLAT.XHOST, PLAT.WEB, OUTR.PROG, OUTR.CONTENT.MIG.PCD, OUTR.CONTENT.MIG.SA, OUTR.CONTENT.TUT.PROBE, OUTR.CONTENT.TUT.NETWORK, OUTR.DOCS.BOOK.PROBE, OUTR.DOCS.BOOK.NETWORK, UX.SCREENS.PROBE, UX.SCREENS.NETWORK, MIG.PROGEQ, FLOW.PROBE, FLOW.NETWORK, FLOW.INSPECT.J7GF]
updated: 2026-11-22
---

# Milestone launch-1: Inspect + Report beachhead

## Goal

Ship the first two focused apps, **Inspect** (scan-to-CAD inspection) and **Report** (transcription and inspection reports, basic SPC), plus the `fid` CLI and MCP servers. Back them with a **published correlation study** against at least one incumbent on public or partner data.

This category is software-only (no motion safety), well understood by buyers, and expensive enough that a free, verifiable alternative gets attention.

## Exit criteria

- These FLOW acceptance jobs pass their gating rules:
  - JOB.ACC.01: scan FAI from CAD + PMI
  - JOB.ACC.02: transcription AS9102
  - JOB.ACC.03: drawing-only FAI
  - JOB.ACC.05: kiosk run
  - JOB.ACC.07: **sections and thickness only**. Gap and flush needs the IND.GAPFL pack (P3) and moves to launch-2.
- MIG correlation study #1 is published under the MIG protocol. Under D-021 option A, the project publishes no comparisons with named incumbents. Study #1 therefore compares results against public reference data (UNC.RDL, NIST/PTB-style sets) and reports parallel-run *method* agreement only. A comparison against a named incumbent can follow later, only when run by a third party with a licence attestation (D-021 option B, after counsel review).
- PLAT Tier 1 platforms are green in CI, and the release ships through ECO.REL with QUAL's SBOM and IQ/OQ pack.
- AGENT's evaluation suite runs against the shipped MCP servers, and the scoreboard is published.

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
