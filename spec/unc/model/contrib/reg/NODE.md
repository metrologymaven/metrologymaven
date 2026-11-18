---
id: UNC.MODEL.CONTRIB.REG
title: Scan registration contributor
level: 4
parent: UNC.MODEL.CONTRIB
kind: task
status: open
phase: P1a
size: L
owners: []
claimed_by:
needs_expert: uncertainty
satisfies: [REQ-UNC.MODEL.CONTRIB-005]
covers: []
depends_on: [UNC.MODEL.CONTRIB.CONTRACT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.CONTRIB.REG: Scan registration contributor

## Purpose

The contribution of scan registration (merging views, or aligning a scan to a reference) to point-based results, taken from the registration residuals. Whether this is a simple residual-based term or should come from transform covariance computed by `UNC.XFM.TCOV` must be settled with the XFM owners and a scanning expert before a leaf is written, so this node stays open.

## Scope

**In:**
- Contributor `uc:scan-registration@<semver>`: inputs residual statistics and point counts from the registration record
- The rule that avoids double counting with transform covariance

**Out:**
- Transform parameter covariance (owned by `UNC.XFM.TCOV`)
- Contract and expansion (owned by `UNC.MODEL.CONTRIB.CONTRACT`)

## Requirements

- **REQ-UNC.MODEL.CONTRIB.REG-001** The contributor shall take the registration record's residual statistics and point counts as inputs, with the registration record as source.
- **REQ-UNC.MODEL.CONTRIB.REG-002** If a transform covariance from `UNC.XFM.TCOV` is bound for the same registration, then the contributor shall add no second registration term and shall return `superseded_by_xfm`.
- **REQ-UNC.MODEL.CONTRIB.REG-003** The contributor shall not reduce the term below the residual RMS by averaging unless the model states its independence assumption [unverified: model choice].

## Interfaces

- **Offers:** contributor `uc:scan-registration@<semver>`.
- **Consumes:** contract (UNC.MODEL.CONTRIB.CONTRACT); registration records from SCAN as artifacts.

## Decomposition hints

One leaf once the XFM boundary and the model are agreed.

## Open questions

- Boundary with `UNC.XFM`: transform contributors are XFM's; a residual-based registration term may double count. Needs agreement with the XFM owners.
- The residual-based model needs review by a scanning expert [unverified].

## Children

_None yet: status `open`._
