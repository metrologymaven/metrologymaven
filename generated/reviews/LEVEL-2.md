# Level-2 review (after the level-1 split)

Date 2026-11-16. Inputs:
- 19 agent expansions (151 subsystems);
- `scope.py check`;
- a cross-domain analysis of dependencies, names and phases.

## Result

- **Tree:** 171 nodes (1 product, 19 domains, 151 subsystems).
- **Validator:** 0 errors. 7 warnings, all phase-order warnings for D-006.
- **Coverage:** 381 of 394 checklist rows covered, 13 out of scope, 0 uncovered.
- **Naming:** no duplicate CLI, MCP or journal names.
- **Sizes:** 91 L, 56 XL, 4 XXL. No leaves yet, as expected at level 2.
- **Blocked:** ECO.APPS is blocked on D-004.

**Update 2026-11-16:** the founder accepted all recommendations. D-001 to D-009 are now `accepted`. HAL, IO and UNC moved to P0, ECO.APPS is unblocked, the DMT is renamed, and the constitution is amended (§2, §3, §3.1, §7).

## Findings and actions

| # | Finding | Action |
|---|---|---|
| 1 | Four domain-level dependency loops: ALN↔GDT, GDT↔UNC, SCAN↔VIZ, ECO↔PLAT. They came from subsystems depending on whole domains. | **Fixed.** Narrowed `depends_on` in ALN.DATUM, GDT.DIM, GDT.DRF, GDT.CHAR, UNC.ORS, ECO.REL, PLAT.DEPLOY/HW/MATRIX/WEB and VIZ.OVERLAY; removed SCAN → VIZ, which pointed the wrong way. `scope.py` now errors on node-level cycles; tested on a planted cycle. |
| 2 | 7 subsystems start earlier than their domain (HAL, IO, UNC) | `scope.py` now warns. **Decision D-006.** |
| 3 | Side-effect classes the constitution doesn't assign: local export, plugin/pack install, job cancel, alarms, MCP dispatcher, single-verb CLI | **Decision D-007**, with proposals |
| 4 | Released-by-other rule and agent run requests for CMM motion | **Decision D-008** |
| 5 | DMT authorship, and its name ("Dialect" vs "defaults" mapping table) | **Decision D-009** |
| 6 | SA.INS.06 (indoor GPS) had no device row | **Fixed.** Added DEV.TST.02 (covered by HAL.DRV's `DEV.TST.*`). |
| 7 | **Ownership gaps.** Each has a proposed owner, to apply when that subsystem is expanded: (a) kernel number formatters `fmt.*` and the exact decimal comparator → KRN.NUM; (b) the REP-1 replay equivalence-tolerance table → UNC.RDL; (c) export of a Command Program to DMIS → CMM.GEN; (d) a lint for product UI strings → ECO.RULES; (e) the Access Ledger (P1b) → CORE.JRN; (f) image-level photogrammetry bundles → LVM.ADJ (later) | **Accepted 2026-11-16.** Each assignment is written into the owning node's Open questions for its expansion. |
| 8 | **Shared boundaries to confirm at level 3.** These are split deliberately and documented on both sides: replay (CORE.RPL engine vs TRUST.REPLAY standalone verifier replayer); curves (KRN.CONS builds section/edge curves, SCAN compares); comparison points (SCAN.CMP owns them and reports through GDT.CHAR); leapfrog (ALN.XFM single transform, HAL device bookkeeping, LVM.MOVE networks); docs (OUTR owns the portal, ECO owns generators and datasets); GOM.EXP.03 PDF (IO.RES envelope vs RPT.OUTPUT) | Accept as is |
| 9 | Values of record from program expressions or scripted checks would break constitution rule C1 | Open question in AUTO.PROG and AUTO.SCRIPTED. A KRN-side expression command may be needed. |
| 10 | Many performance and tolerance numbers are marked `[unverified]` and depend on PLAT.HW reference machines | Expected. They get measured once PLAT.HW defines the machines. |
