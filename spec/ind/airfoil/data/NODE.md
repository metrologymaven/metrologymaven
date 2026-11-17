---
id: IND.AIRFOIL.DATA
title: Airfoil method declarations and reference datasets
level: 3
parent: IND.AIRFOIL
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.AIRFOIL-007, REQ-IND.AIRFOIL-008]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, IND.CONTRACT.VERIFY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.AIRFOIL.DATA: Airfoil method declarations and reference datasets

## Purpose

The airfoil pack's manifest content and its reference datasets: versioned method IDs for every parameter definition, the named maintainer, and the datasets with expected results that `fid pack verify-pack` runs.

## Scope

**In:**
- Manifest entries: method IDs `pack.airfoil.<parameter>.<method>@<semver>`, maintainer, no standards declared unless a maintainer identifies one
- Analytic datasets from closed-form profiles (for example a symmetric and a cambered NACA 4-digit section [unverified suitability]) sampled densely, with analytic expected values
- One measured blade section from a licence-clear source
- A 100 000-point benchmark section for the performance requirement
- Expected-results files in `fid.pack.expected@1`

**Out** (and which node owns it instead):
- The harness that runs them (owned by `IND.CONTRACT.VERIFY`)
- The algorithms under test (owned by `IND.AIRFOIL.PARAMS`, `IND.AIRFOIL.FORM`, `IND.AIRFOIL.STACK`)

## Requirements

- **REQ-IND.AIRFOIL.DATA-001** The airfoil pack manifest shall declare every parameter definition as a versioned method ID and shall name at least one maintainer.
- **REQ-IND.AIRFOIL.DATA-002** The pack shall ship at least a symmetric analytic profile, a cambered analytic profile and a measured blade section as reference datasets.
- **REQ-IND.AIRFOIL.DATA-003** On the analytic datasets, the expected value of every parameter shall be derived from the closed-form profile, with tolerance 1e-9 m for lengths and 1e-9 rad for angles [unverified achievable with the chosen sampling].
- **REQ-IND.AIRFOIL.DATA-004** Each dataset shall state its licence (source licence or CC-BY-4.0) and its generator script and seed where synthetic.

## Interfaces

- **Offers:** manifest fragment and dataset folder for the airfoil pack.
- **Consumes:** `IND.CONTRACT.MANIFEST` schema; `IND.CONTRACT.VERIFY` expected-results format.

## Decomposition hints

SYNTH (analytic generators and expected values), MEAS (measured section sourcing and licensing).

## Open questions

- A licence-clear measured blade section has not been identified; a maintainer or partner must supply one.

## Children

_None yet: status `open`._
