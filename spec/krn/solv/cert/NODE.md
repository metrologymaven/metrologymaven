---
id: KRN.SOLV.CERT
title: Active-set certificates, recomputation and tie-break
level: 3
parent: KRN.SOLV
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-004, REQ-KRN.SOLV-005, REQ-KRN.SOLV-006, REQ-KRN.SOLV-007]
covers: []
depends_on: [KRN.NUM.LA.QR, KRN.NUM.RED, KRN.SOLV.CONIC.WRAP, CORE.JRN.CANON]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CERT: Active-set certificates, recomputation and tie-break

## Purpose

Solver iterates are not results. This module takes a converged conic solution, extracts the active (contact) set, recomputes the reported optimum from that set with a small deterministic direct solve, checks the optimality (KKT) conditions independently, applies a documented tie-break for degenerate or tied sets, and packages a certificate a verifier can recheck in microseconds. Two iterate paths reaching the same active set therefore give bit-identical reported values.

## Scope

**In:**
- Active-set extraction from primal slacks and dual multipliers with explicit thresholds
- The `ActiveSetProblem` callback: given an active set, form and solve the equality system that defines the optimum (for example equal deviations at the critical points)
- Independent KKT check (primal feasibility, dual feasibility, complementarity, stationarity) with recorded residuals
- Deterministic tie-break for degenerate or tied active sets (lowest point indices in total order) and the `tie_broken` flag
- The `Certificate` type: kind, active set, multipliers, KKT residuals, condition number, flags (`tie_broken`, `local_optimum_only`), digest
- The rule that only recomputed values, never iterates, enter output digests

**Out** (and which node owns it instead):
- Producing the conic solution (`KRN.SOLV.CONIC`) and the linearisation loop (`KRN.SOLV.SEQ`)
- Least-squares diagnostics, which need no active set (`KRN.SOLV.NLLS`)
- Per-primitive active-set equations (`KRN.MZ`, `KRN.ENV` implement the callback)
- Verifier-side certificate rechecking (`TRUST`)

## Requirements

- **REQ-KRN.SOLV.CERT-001** The module shall extract the active set from a conic solution using explicit, recorded slack and multiplier thresholds.
- **REQ-KRN.SOLV.CERT-002** The module shall recompute the reported optimum from the active set by a deterministic direct solve through `fid-num`, and the recomputed value shall depend only on the active set and the input data.
- **REQ-KRN.SOLV.CERT-003** The module shall check KKT conditions on the recomputed point independently of the solver, and shall return a typed `certificate_failed` error if any residual exceeds its recorded tolerance.
- **REQ-KRN.SOLV.CERT-004** When more than one active set satisfies the thresholds, the module shall select the one whose sorted point indices are lowest in lexicographic order and set `tie_broken = true`.
- **REQ-KRN.SOLV.CERT-005** The output digest of a certified result shall be computed only from the recomputed values and the certificate fields, never from solver iterates.
- **REQ-KRN.SOLV.CERT-006** For a frozen set of problems solved from two different starting points that reach the same active set, the reported values shall be bit-identical.

## Interfaces

- **Offers:** `fid_solv::cert::{certify, ActiveSetProblem, Certificate, CertFlags}`; certificates carried by `KRN.MZ`, `KRN.ENV`, `GDT` results and rechecked by `TRUST`.
- **Consumes:** `KRN.SOLV.CONIC.WRAP` solutions, `KRN.NUM.LA.QR`, `KRN.NUM.RED`, and `CORE.JRN.CANON` (L0) for the certificate digest.

## Decomposition hints

AS (active-set extraction and thresholds), RECOMP (the callback and direct solve), KKT (independent optimality check), TIE (tie-break and degenerate cases), CT (certificate type, serialisation and digest rule).

## Open questions

- Certificate serialisation must match what the verifier in `TRUST` rechecks; agree on the schema when `TRUST.VERIFY` is expanded. Carried into `KRN.SOLV.CERT.CT` as schema `fid.cert@1`.
- **Applied at split (2026-11-17):** nonlinear active-set equations (circles, cylinders) are solved from a start derived from the set itself (`start_from_set`), so the recomputed value cannot depend on the iterate (`KRN.SOLV.CERT.RECOMP`). Callers already written against this interface (`GDT.ZONE.POSE.LIN`, `KRN.MZ.LIN`) need to provide that method.

## Children

The split follows the hints, with the tie-break folded into extraction (it is the order in which candidates are listed) and the pipeline placed with the certificate type, so the three steps below it have no dependency on the certificate.

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.SOLV.CERT.AS | Active-set extraction, candidate enumeration and tie-break order | 001 004 | none |
| KRN.SOLV.CERT.RECOMP | ActiveSetProblem callback and deterministic recomputation | 002 006 | none |
| KRN.SOLV.CERT.KKT | Independent KKT optimality check at the recomputed point | 003 | none |
| KRN.SOLV.CERT.CT | Certificate type, certify pipeline and digest rule | 003 004 005 006 | none |
