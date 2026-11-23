---
id: D-033
title: Which incumbent program and project formats migration may read
status: accepted
raised_by: MIG split
blocks: [MIG.PROGEQ]
decide_by: before MIG.SRC admits any incumbent-native format (P1a) and before MIG.PROGEQ grows its incumbent-format child (P4)
updated: 2026-11-22
---

# D-033: Which incumbent program and project formats migration may read

## Question

Which incumbent-native program and project formats (for example PC-DMIS and CALYPSO programs, PolyWorks and ZEISS INSPECT projects) may the project read for migration, and by which route: a published specification, a user-exported open format only, or a clean-room reader built from observed behaviour?

## Context

REQ-MIG-007 requires that migration tooling respects incumbent EULAs and the clean-room rule (CONSTITUTION §8), and that formats which cannot be read cleanly are supported only through user-exported open formats. Whether a given native format has a public specification, and whether its licence terms forbid reverse engineering or interoperability work, is not verified [unverified for every incumbent]. Interoperability exceptions in some jurisdictions may allow more than the licence text suggests [unverified], which is a counsel question, not an engineering one.

`MIG.SRC` records admission outcomes but cannot decide them. Blocks the incumbent-format half of `MIG.PROGEQ` (its planned INCUMB child, P4) and limits `MIG.PROJ` and `MIG.TEMPL` to open exports until decided. DMIS 5.3, QIF, AQDEF and CSV routes are not blocked.

## Options

| Option | For | Against |
|---|---|---|
| A. Open exports only: never read incumbent-native formats; users export DMIS, QIF, CSV or AQDEF from their incumbent | No licence or clean-room risk; simplest | Program migration from incumbents without good DMIS export stays manual; visible parity gap |
| B. Per-format counsel review: admit a native format only with a public specification or written counsel clearance, recorded in `MIG.SRC` | Opens formats that are legitimately documented | Counsel cost per format; slow; reviews must be repeated as licence terms change |
| C. B plus vendor cooperation: admit formats whose vendor provides a documented export or SDK under terms compatible with D-001 | Best fidelity where vendors cooperate | Depends on vendors; SDK terms may be incompatible with the licence outcome |

## Recommendation

A for P1a through P2 (DMIS and open exports cover stages 1 to 5 in part), then B per format at P4 when program conversion from incumbents is scheduled. [unverified: availability of public specifications]

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

A for P1a through P2 (DMIS and open exports cover stages 1 to 5 in part), then B per format at P4 when program conversion from incumbents is scheduled. [unverified: availability of public specifications]

**Condition:** the counsel review named in the recommendation must happen before the affected node is published or released.

**Amended 2026-11-22 by D-052:** Option A is permanent (open metrology standards only: QIF 3.0, STEP AP242, DMIS, I++ DME, STL/PLY/OBJ/E57/LAS/CSV/PDF; proprietary incumbent format readers and counsel condition are dropped).
