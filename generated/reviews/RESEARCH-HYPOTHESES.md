# Research hypotheses and reference-data gaps (D-036 rule 4)

Date 2026-11-17. Six agent passes added measurable hypotheses to the research-risk nodes. The result is **29 nodes with `## Research hypotheses` sections and 122 `AC-H*` acceptance criteria**, each in the form "method X achieves ≤ Y against reference set Z, measured by W; falsified if V".

Thresholds are **targets, not results**. Every threshold that could not be sourced is marked `[unverified]`.

## Research-risk areas

| Area | Nodes | What the hypotheses claim (summary) |
|---|---|---|
| Kernel fit validation | KRN.MAN.VAL (4) | Least-squares and minimum-zone fits agree with NIST ATS / PTB softgauge data and with project null-space generators within `rdl-mpe@1`; seeded faults at 2×MPE are caught 100 %; an independent solver agrees; reports reproduce on 3 OSes |
| Task-specific uncertainty | UNC.PROP, UNC.MODEL.SUBST (27) | LPU and MCM coverage stays inside an exact 99 % binomial band around 0.95 on known-truth cases; JCGM 100/101/102 worked examples reproduced to published digits; multivariate and datum-correlated cases covered |
| Open GD&T and datum reference suites | UNC.ORS (32) | Closed-form agreement with an independent arbitrary-precision oracle (≤ 1e-12 m); ≥ 90–95 % discrimination of seeded faults; coverage of the callout types in NIST MBE PMI models; the ASME/ISO pairing is explicit |
| Independent replay | TRUST.REPLAY (9) | D0 results replay IDENTICAL on 100 % of the frozen corpus on 6 host targets; zero false DIVERGENT and zero false acceptance; the standalone verifier agrees with CORE replay; the sandbox is contained |
| Bounded agent autonomy | AGENT.REDTEAM (8) | Zero gate bypasses in ≥ 59 runs per attack category (one-sided 95 % upper bound ≤ 5 %) across motion, release, adoption, admin, egress and injection; the verdict is never `pass` on seeded failing sets |

## The research finding

There is **no public national-metrology-institute reference data** with expected results for:
- GD&T evaluation (datum reference frames, position with material modifiers, profile, composite, simultaneous requirements);
- ISO 14405-1 size modifiers and the ASME envelope requirement;
- alignment (3-2-1, RPS, constrained best-fit);
- outlier rejection, resampling and point-to-CAD projection;
- task-specific uncertainty for heterogeneous point data, and substitution-method (ISO 15530-3) workpieces.

The project's own openly licensed generators, independent oracle and reference suites would be the first such public references. Producing and validating them is unproven research, not routine engineering. This is NSF aim 1 (open GD&T/datum reference suites) and aim 2 (task-specific uncertainty), already named in the concept docs.

## Gaps to close (owners)

| Gap | Owner |
|---|---|
| Minimum-zone generator family, including a 2D line (`mz.line`) | UNC.RDL.GEN.MZEQ (open) |
| NIST ATS and PTB softgauge importers; versions and licences unconfirmed | UNC.RDL.EXT.NIST, UNC.RDL.EXT.PTBSG (open) |
| TraCIM test-data redistribution terms; automated submission terms | UNC.RDL.TRACIM.PROTO (open); people decision |
| Filter reference softgauges (NIST SMRS, type F1/F2) have no importer | UNC.RDL.EXT (new child needed) |
| A seeded-fault (mutant) catalogue that the discrimination hypotheses rely on | UNC.ORS.RUN or UNC.ORS.TRACE (needs owner) |
| A generator for the substitution method (calibrated workpieces) | UNC.RDL.GEN or UNC.ORS.UNC (new child needed) |
| A construction family for Jacobian tests | UNC.RDL.GEN (new child needed) |
| JCGM worked examples as fixtures, with a licence-evidence record | UNC.RDL.EXT (new child needed) |
| A replay corpus pinned to the first released kernel | TRUST.REPLAY.CORPUS, after KRN 1.0 |
| A red-team child for H3/H4 gates and injected UI input end to end | AGENT.REDTEAM (new child needed) |
| The hostile-module set for sandbox containment | TRUST.REPLAY.SANDBOX |
| Ratifying the REP-1 replay tolerances | UNC.RDL.REP |
| Who pays for agent red-team model runs at the required volume | People decision (same as AGENT.BOARD) |

Publication of the project-authored suites is held until D-001's counsel conditions and D-002's name clear (D-036 rule 5).
