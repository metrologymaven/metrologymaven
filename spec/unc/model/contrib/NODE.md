---
id: UNC.MODEL.CONTRIB
title: Contributor plug-in contract and non-instrument contributor library
level: 3
parent: UNC.MODEL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL-005]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.MODEL.DIST]
cli: [fid unc list-contributor, fid unc show-contributor]
mcp: [unc_list_contributor, unc_show_contributor]
journal: [unc.list_contributor@1]
updated: 2026-11-16
---

# UNC.MODEL.CONTRIB: Contributor plug-in contract and non-instrument contributor library

## Purpose

A contributor model is a reusable sub-graph that turns physical facts (temperatures, CTE, stylus qualification, sampling density, registration residuals) into input quantities of an Uncertainty Model. This node defines the plug-in contract that every contributor follows and ships the first-party library of non-instrument contributors. Instrument contributors are a separate data product in `UNC.INST`.

## Scope

**In:**
- Contributor contract: ID `uc:<name>@<semver>`, declared parameters with units, the sub-graph it expands to, required source kinds, applicability limits, references by designator and clause
- Library: thermal expansion (temperature sensor uncertainty, CTE uncertainty, differential expansion part versus scale), probe and stylus qualification, sampling and unsampled form, scan registration residual
- `unc.list_contributor@1` and `unc.show_contributor@1` (R0)

**Out:**
- Instrument error models and MPE-derived terms (owned by `UNC.INST`)
- Graph validation (owned by `UNC.MODEL.GRAPH`)
- Contributors that come from transforms (owned by `UNC.XFM`)

## Requirements

- **REQ-UNC.MODEL.CONTRIB-001** Every contributor shall declare an ID with semver, its parameters with units and valid ranges, the source kinds each parameter requires, and the sub-graph it expands to.
- **REQ-UNC.MODEL.CONTRIB-002** When a contributor is expanded into a model, the result shall pass `UNC.MODEL.GRAPH` validation, and if it does not, then the expansion shall fail with error `contributor_invalid`.
- **REQ-UNC.MODEL.CONTRIB-003** The thermal contributor shall produce a length uncertainty from nominal length, temperature deviation from 20 °C with its uncertainty, and CTE with its uncertainty, for part and scale separately.
- **REQ-UNC.MODEL.CONTRIB-004** The sampling contributor shall give a non-zero contribution for form-dependent characteristics when the number of points is below a configured density, and shall state the form model used [unverified: choice of model].
- **REQ-UNC.MODEL.CONTRIB-005** The library shall contain at least the thermal, stylus-qualification, sampling and scan-registration contributors at first release [unverified: completeness against ISO 15530 contributor lists].
- **REQ-UNC.MODEL.CONTRIB-006** If a contributor parameter lies outside its declared valid range, then expansion shall fail with `out_of_range` and shall not extrapolate.

## Interfaces

- **Offers:** `ContributorModel` contract; `expand(contributor, params) -> SubGraph`; commands `unc.list_contributor@1`, `unc.show_contributor@1`; library data files.
- **Consumes:** graph types (UNC.MODEL.GRAPH); distributions (UNC.MODEL.DIST).

## Decomposition hints

CONTRACT (plug-in contract and expansion), THERM, STYLUS, SAMPLE, REG (one leaf per library contributor), LIST (the two read commands).

## Open questions

- The unsampled-form model (for example a harmonic or Gaussian-process prior) needs a metrology review before it is fixed [unverified].
- Whether third-party contributor plug-ins are code (A0 install) or data only should follow ECO.SDK; this node assumes data-only sub-graphs.

## Children

_None yet: status `open`._
