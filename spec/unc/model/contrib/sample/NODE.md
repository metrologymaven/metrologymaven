---
id: UNC.MODEL.CONTRIB.SAMPLE
title: Sampling and unsampled-form contributor
level: 4
parent: UNC.MODEL.CONTRIB
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.CONTRIB-004, REQ-UNC.MODEL.CONTRIB-005]
covers: []
depends_on: [UNC.MODEL.CONTRIB.CONTRACT]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# UNC.MODEL.CONTRIB.SAMPLE: Sampling and unsampled-form contributor

## Purpose

The contribution of sparse sampling to form-dependent characteristics: when fewer points than a configured density are taken, part of the true form error lies between the points and is missed. The contributor needs a model of the unsampled form (for example a harmonic band limit or a Gaussian-process prior), which must be chosen with a metrology review before it is fixed, so this node stays open.

## Scope

**In:**
- Contributor `uc:sampling@<semver>`: inputs point count, feature size, configured density, form model parameters
- Validation on synthetic parts with known form

**Out:**
- Contract and expansion (owned by `UNC.MODEL.CONTRIB.CONTRACT`)
- Sampling strategy planning (owned by `CMM.STRAT` and `UNC.PLAN`)

## Requirements

- **REQ-UNC.MODEL.CONTRIB.SAMPLE-001** When the point count on a form-dependent characteristic is below the configured density, the contributor shall give a non-zero contribution and state the form model and its parameters.
- **REQ-UNC.MODEL.CONTRIB.SAMPLE-002** When the configured density is met, the contributor shall return zero contribution with reason `density_met`, and it shall never return a negative variance.
- **REQ-UNC.MODEL.CONTRIB.SAMPLE-003** The form model shall be validated on synthetic parts with known form (for example lobed circles) before release, with the results recorded [unverified: validation set].

## Interfaces

- **Offers:** contributor `uc:sampling@<semver>`.
- **Consumes:** contract and expansion (UNC.MODEL.CONTRIB.CONTRACT).

## Decomposition hints

MODEL (form model choice and kernel operator, after review), VALID (synthetic validation set).

## Open questions

- The unsampled-form model (harmonic band limit, Gaussian-process prior or another) needs a metrology review before it is fixed [unverified].
- Default density thresholds per feature type should come from the same review.

## Children

_None yet: status `open`._
