---
id: KRN.ENV.MCC
title: Minimum circumscribed fits
level: 3
parent: KRN.ENV
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.ENV-001, REQ-KRN.ENV-003, REQ-KRN.ENV-004, REQ-KRN.ENV-005, REQ-KRN.ENV-009]
covers: [GOM.CON.20, PW.CON.05]
depends_on: [KRN.LSQ.RND, KRN.LSQ.AXIS, KRN.SOLV.CONIC, KRN.SOLV.SEQ, KRN.SOLV.CERT, KRN.SOLV.MSTART]
cli: [fid fit mcc-circle2d, fid fit mcc-sphere, fid fit mcc-cylinder]
mcp: [fit_mcc_circle2d, fit_mcc_sphere, fit_mcc_cylinder]
journal: [fit.mcc_circle2d@1, fit.mcc_sphere@1, fit.mcc_cylinder@1]
updated: 2026-11-16
---

# KRN.ENV.MCC: Minimum circumscribed fits

## Purpose

The smallest circle, sphere or cylinder that contains all points: the mating envelope of a shaft. For circle and sphere the problem is a convex SOCP whose global optimum can be certified by a duality gap; for the cylinder it is non-convex in the axis and needs sequential linearisation and multi-start. Slot and rectangle envelopes are included where defined.

## Scope

**In:**
- `fit.circle2d.mcc`, `fit.sphere.mcc` as SOCP with duality-gap certificate
- `fit.cylinder.mcc` by sequential linearisation from the least-squares axis, with multi-start and `local_optimum_only`
- Slot and rectangle circumscribed envelopes where defined, otherwise `method_undefined`
- Registry commands and spec cards

**Out** (and which node owns it instead):
- Welzl cross-check (`KRN.ENV.XCHK`)
- Rule #1 and MMC bonus logic using the envelope (`GDT`)

## Requirements

- **REQ-KRN.ENV.MCC-001** The module shall compute minimum circumscribed 2D circle, sphere and cylinder fits implementing the `Fit` trait, each returning the contact set and a certificate.
- **REQ-KRN.ENV.MCC-002** For the 2D circle and sphere, the result shall carry a duality gap below 1e-12 m [unverified] and the radius recomputed from the contact set.
- **REQ-KRN.ENV.MCC-003** For the cylinder, the result shall carry the multi-start agreement flag, a local optimality check and `local_optimum_only` when global optimality is not proven.
- **REQ-KRN.ENV.MCC-004** If a circumscribed fit is requested for an undefined primitive (for example an open line), then the system shall return `method_undefined`.
- **REQ-KRN.ENV.MCC-005** When a command is committed, the system shall journal one `fit.mcc_<primitive>@1` event with algorithm ID, parameter digest, input digest, seed and output digest.

## Interfaces

- **Offers:** commands `fit.mcc_circle2d@1`, `fit.mcc_sphere@1`, `fit.mcc_cylinder@1` (W2, dry-run R1); envelopes with flags for `GDT`.
- **Consumes:** `KRN.LSQ`, `KRN.SOLV`.

## Decomposition hints

CVX (circle and sphere SOCP with duality gap), CYL (cylinder, non-convex), PLAN (slot and rectangle envelopes).

## Open questions

- Whether slot and rectangle circumscribed envelopes have an accepted definition [unverified]; if not, they return `method_undefined`.

## Children

_None yet: status `open`._
