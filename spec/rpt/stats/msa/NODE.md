---
id: RPT.STATS.MSA
title: Measurement Systems Analysis, VDA 5 and non-normal capability
level: 3
parent: RPT.STATS
kind: module
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-RPT.STATS-002, REQ-RPT.STATS-014, REQ-RPT.STATS-015]
covers: []
depends_on: [RPT.STATS.DSET, KRN.NUM, UNC.UEC, UNC.MODEL]
cli: [fid stats compute-msa, fid stats compute-nonnormal]
mcp: [stats_compute_msa, stats_compute_nonnormal]
journal: [stats.compute_msa, stats.compute_nonnormal]
updated: 2026-11-22
---

# RPT.STATS.MSA: Measurement Systems Analysis, VDA 5 and non-normal capability

## Purpose

Post-1.0 statistical expansion adopted under D-054: Measurement Systems Analysis (AIAG MSA 4th Ed. and VDA 5 capability indices bound directly to `UNC` budgets) plus non-normal process capability indices (ISO 22514-2) needed for geometric tolerances whose distributions are inherently one-sided or skewed (position, form, runout).

## Scope

**In:**
- AIAG MSA 4th Ed. Type 1 gauge studies (`Cg`, `Cgk`, bias, repeatability)
- Crossed and nested ANOVA Gage R&R (`%EV`, `%AV`, `%GRR`, `%PV`, `%SV`, `%Tol`, `ndc`, operator-by-part interaction) and Average-and-Range method
- Linearity, bias, stability and destructive MSA studies
- Attribute agreement analysis (Cohen/Fleiss kappa, repeatability, effectiveness vs. reference)
- VDA 5 measurement-process capability (`Q_MS`, `Q_MP`) linked to `UNC` uncertainty statements
- ISO 22514-2 non-normal capability distributions (Weibull, Rayleigh, folded-normal, lognormal, Box-Cox and percentile methods)

**Out** (and which node owns it instead):
- Normal Cp/Cpk/Pp/Ppk (`RPT.STATS.CAP`)
- Data-set selection and exclusions (`RPT.STATS.DSET`)
- Control charts and run rules (`RPT.STATS.SPC`)
- Instrument uncertainty propagation itself (`UNC.PROP`)

## Requirements

- **REQ-RPT.STATS.MSA-001** The system shall compute Type 1 gauge studies (`Cg`, `Cgk`), crossed and nested ANOVA Gage R&R, linearity, bias and attribute agreement studies in the deterministic CPU kernel (KNP-1, D1 or better).
- **REQ-RPT.STATS.MSA-002** The system shall compute VDA 5 `Q_MS` and `Q_MP` ratios directly from `UNC` expanded uncertainty statements and characteristic tolerance spans, refusing to fabricate uncertainty when UEC-0 is present.
- **REQ-RPT.STATS.MSA-003** The system shall compute non-normal capability indices under ISO 22514-2 (percentile, Weibull, Rayleigh, folded-normal and Box-Cox) with goodness-of-fit diagnostics and explicit method citation.
- **REQ-RPT.STATS.MSA-004** Every study result shall cite its governing standard designator, edition and clause without embedding standards text, and shall carry `interpretation: provisional` where defaults have not received H4 sign-off (D-045).

## Interfaces

- **Offers:** `stats.compute_msa@1`, `stats.compute_nonnormal@1` (R1 compute, W2 when committed to study record); `MsaStudy` and `NonNormalCapability` payloads for `RPT.MODEL`.
- **Consumes:** `RPT.STATS.DSET` (`DataSet`), `KRN.NUM`, `UNC.UEC`, `UNC.MODEL`.

## Decomposition hints

TYPE1 (Type 1 & linearity/bias), GRR (crossed/nested ANOVA & A&R Gage R&R), ATTR (attribute agreement), VDA5 (VDA 5 capability tied to UNC), NONNORM (ISO 22514-2 non-normal distributions).

## Open questions

- Split during P3 wave expansion under D-052; excluded from `launch-1` per D-054.

## Children

_None yet: status `open`._
