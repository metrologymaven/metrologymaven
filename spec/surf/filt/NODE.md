---
id: SURF.FILT
title: ISO 16610 profile and areal filtration engine
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: XL
owners: []
claimed_by:
satisfies: [REQ-SURF-001]
covers: []
depends_on: [KRN.NUM, HAL.IDL.SENS]
cli: [fid surf filter-*]
mcp: [surf_filter_*]
journal: [surf.filter_*]
updated: 2026-11-22
---

# SURF.FILT: ISO 16610 profile and areal filtration engine

## Purpose

Pure-Rust KNP-1 filtration library implementing ISO 16610 profile and areal filters (linear Gaussian, spline, robust Gaussian, robust spline and morphological closing/opening) with explicit nesting indices (`λs`, `λc`, `λf`), end-effect policies and double-filter refusal.

## Scope

**In:** ISO 16610-21/22/31/32/40/41/49 profile filters and ISO 16610-61/62/71/81/85 areal filters; F-operator form removal; S-filter and L-filter chains; validity-mask handling; double-filtering guard reading `Profile@1` / `HeightMap@1` acquisition filter metadata.
**Out:** Parameter evaluation (`SURF.PROF`, `SURF.AREAL`, `SURF.FORM`); point-cloud outlier policies (`KRN.OUT`, `SCAN.CLOUD`).

## Requirements

- **REQ-SURF.FILT-001** Every filter shall execute deterministically in pure-Rust `f64` under KNP-1 (D0/D1) with explicit nesting indices and boundary policies recorded in the result digest.
- **REQ-SURF.FILT-002** If an input trace declares an upstream instrument filter overlapping the requested cutoff band, then the filter command shall refuse execution with a typed double-filter error unless an explicit override policy is journaled.
- **REQ-SURF.FILT-003** Linear filter transmission functions on closed-form sinusoids shall agree with analytic transmission targets within 1 nm [unverified] across the `UNC.ORS.FLT` test wavelengths.

## Children

_None yet: status `open`._
