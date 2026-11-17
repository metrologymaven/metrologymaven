---
id: KRN.SOLV.SEQ
title: Sequential linearisation driver
level: 3
parent: KRN.SOLV
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV-001, REQ-KRN.SOLV-008, REQ-KRN.SOLV-009]
covers: []
depends_on: [KRN.SOLV.CONIC, KRN.SOLV.CERT, KRN.SOLV.MSTART]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.SOLV.SEQ: Sequential linearisation driver

## Purpose

Minimum-zone, maximum-inscribed and tangential problems for curved features are non-convex in the axis or centre. This driver linearises such a problem about the current estimate, solves the resulting LP or SOCP with a step bound, updates, and repeats until the active set is stable. It is generic over a problem callback, so `KRN.MZ`, `KRN.ENV` and `GDT` share one loop.

## Scope

**In:**
- The `LinearisableProblem` callback: build the local LP or SOCP about a point, and evaluate the true objective and constraints
- The iteration loop with a step bound (trust region), fixed acceptance and radius-update rules
- Stop rules: active set unchanged for a fixed number of iterations and step below tolerance, or iteration limit
- Hand-off of the final active set to `KRN.SOLV.CERT`
- Optional outer trust region supplied by the caller (for example the MIC centre region from `KRN.ENV`)

**Out** (and which node owns it instead):
- The conic solves themselves (`KRN.SOLV.CONIC`)
- Certificates and recomputation (`KRN.SOLV.CERT`)
- Choosing and running multiple starts (`KRN.SOLV.MSTART`)
- Problem formulations per primitive (`KRN.MZ`, `KRN.ENV`)

## Requirements

- **REQ-KRN.SOLV.SEQ-001** The driver shall iterate linearise-solve-update from a caller-supplied start, with a step bound whose update rule is a fixed documented function of the ratio of actual to predicted improvement.
- **REQ-KRN.SOLV.SEQ-002** The driver shall stop with success only when the active set is unchanged for 3 [unverified] consecutive iterations and the step norm is below the configured tolerance.
- **REQ-KRN.SOLV.SEQ-003** If the iteration limit is reached, a subproblem is infeasible, or the step bound shrinks below its minimum, then the driver shall return a typed `SolverError` with the last active set and diagnostics, and no value.
- **REQ-KRN.SOLV.SEQ-004** If the caller supplies an outer trust region and the converged point lies on its boundary, then the driver shall report `on_trust_boundary` so the caller can raise its own typed error.
- **REQ-KRN.SOLV.SEQ-005** On success, the driver shall pass the final active set to `KRN.SOLV.CERT` and return only the certified result, never its own last iterate.

## Interfaces

- **Offers:** `fid_solv::seq::{solve, LinearisableProblem, SeqConfig, SeqOutcome}`.
- **Consumes:** `KRN.SOLV.CONIC`, `KRN.SOLV.CERT`; called through `KRN.SOLV.MSTART` for multi-start.

## Decomposition hints

LOOP (iteration, step bound and stop rules), TR (outer trust region and boundary detection), TEST (non-convex reference problems: MZ circle, MIC circle, cylinder axis).

## Open questions

- The active-set stability count and step tolerance may need to be per quantity kind (REP-1 table in the concept doc) [unverified].

## Children

_None yet: status `open`._
