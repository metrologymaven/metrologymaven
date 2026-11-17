# Level-3 review (after the level-2 split)

Date 2026-11-16. Branch `grow/level-2`. Inputs:
- 19 domain agents' reports;
- `scope.py check`;
- the `launch-1` milestone plan.

## Result

| | |
|---|---|
| Nodes | 1,037 (before the 7 new domains are split) |
| Buildable leaves | 383 |
| Open nodes | 456 |
| Blocked on decisions | 14 |
| Validator | 0 errors; 2 warnings (two size-M non-leaves in UNC, intentionally kept open) |
| Checklist coverage | 423 rows (competitors, devices, journeys); 0 uncovered |
| Decisions | D-010 to D-033 opened by agents (D-030, D-032, D-033 in the new domains); D-031 and D-034 opened by this review |
| launch-1 | 322 leaves in scope; 305 ordered in 63 build waves; 17 in a TRUST-internal loop; 308 nodes still to expand; 11 blocking decisions |

## Fixed in this review

- **Duplicate DMIS results command:** removed. One command remains, `import.results_dmis@1`, in IO.RES.TRANS.
- **Leaves with two commands:** a leaf can now declare one side-effect class per command (`side_effect: [W2, W1]`), for format leaves that have both import and export.
- **Smaller adoption programs added** as requirements plus open modules:
  - OUTR.PROG.CERT: training and certification
  - OUTR.PROG.STDS: standards-body participation
  - OUTR.PROG.LAB: partner labs, hardware-in-the-loop access and round-robins
  - ECO.COMM.PARTNER: integrator and support partners
  - ECO.REL.LTS: long-term-support channel
  - PLAT.DOCTOR.FEEDBK: opt-in crash and issue reports
- **Decision numbering:** collisions were renumbered by the agents themselves. D-010 to D-029 are unique.
- **New tooling:**
  - `uses_artifacts:` frontmatter (D-034)
  - `scope.py milestone` (dependency closure, build waves, open work, blocking decisions)

## Structural findings, for decision

| # | Finding | Proposed resolution |
|---|---|---|
| 1 | Dependency loops are being dodged with prose "interface notes" in at least 9 domains | **D-034** layers + `uses_artifacts:` |
| 2 | The same constitution gaps were hit repeatedly: journaling of R0/R1, human-only commands, hold-request class, consent class, generic egress | **D-031** |
| 3 | The TRUST subsystems depend on each other at subsystem level (the 17 unordered leaves in launch-1) | At the next TRUST expansion, narrow `depends_on` to leaf level; the shared verification-report schema becomes an artifact (`uses_artifacts`) |
| 4 | D-012: whether a person at the CLI counts as `human` or `script`. It blocks CORE.BUS.PRIN and AUTO.PY.RUN, and affects signing from the CLI | Decide first: it is the most-cited open decision |

## Ownership conflicts (resolve at the next expansion of the named nodes)

| Conflict | Proposed owner |
|---|---|
| Gauge register: HAL.DRV vs RPT.BALLOON.GAUGE vs UNC.INST | **UNC.INST.LIB** owns the instrument and gauge register (identity, resolution, MPE, calibration due). HAL and RPT reference it. |
| Unit catalogue: CORE.REG.DEF vs GEO.FRAME.UNITS | One L0 crate owned by **GEO.FRAME.UNITS** (D-034 Foundation). CORE.REG.DEF references it. |
| Feature-type inference: CMM.STRAT.LEARN vs IND.PORT.RECOG | **KRN** (D-029, recommendation A) |
| Vocabulary lints: OUTR.BRAND.LINT vs ECO.RULES.VOCAB | One engine in **ECO.RULES.VOCAB**. OUTR supplies the outreach rule set. |
| Benchmark suite: ECO.DATA.BENCH vs PLAT.HW.SUITE | PLAT.HW owns suite content and the result schema; ECO.DATA.BENCH is the runner (artifact dependency) |
| Datum DoF bookkeeping: ALN.DATUM.DOF vs GDT.DRF | **GDT.DRF** owns one DoF table. ALN.DATUM.SYS calls GDT.DRF's R1 entry point. |
| Comparison-point model: SCAN.CMP.CPT vs GDT.CHAR | SCAN owns the definition and GDT.CHAR accepts it through the QIF attributes hook (as SCAN proposed) |
| Kernel replay tolerances: TRUST.REPLAY vs UNC.RDL.REP | **UNC.RDL.REP** (`rep-1`). TRUST and CORE reference it. |
| Drawing PDF and raster reader for ballooning (RPT.BALLOON.SHEET) | **IO**: a new IO.REG child or format plugin |
| R1 binning for colour maps (VIZ vs SCAN) | **SCAN.CMP.BAND** (`compare.surface_bin@1`) |
| Crate naming convention | **ECO.REPO.TOPO** publishes the crate map; current agent names (`fid-gps`, `fid-geo-*`, `fid-scan-*`, `fid-cmm-*`) are proposals |

## Phase notes (D-006 rule: a phase means work starts)

- **ECO.APPS proposes app phases:** Report P1a, Inspect P1b, Probe P2, Network P3. These match launch-1.
- **D-018** (CMM children later than P2) and **D-025** (verifier chain-only path in P0) are open.
- **Possible earlier phases:** GEO.DIST.MESH and GEO.OOC.LOD may be needed in P1a by ALN and VIZ.
- **Device seed mismatch:** DEV.ARE.01 is P1b in the device catalog, but it is delivered by IO and HAL.ACQ.FILEDEV in P1a; HAL.DRV is P2. Accept as is: the file path is the P1b deliverable.

## Open decisions (D-010 to D-034)

The recommendations are summarised in `generated/DECISIONS.md`. The ones that block launch-1:
D-011, D-012, D-013, D-015, D-016, D-017, D-019, D-020, D-021, D-024, D-028.
