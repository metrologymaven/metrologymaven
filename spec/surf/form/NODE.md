---
id: SURF.FORM
title: Precision roundness, harmonic UPR and spindle separation
level: 2
parent: SURF
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-SURF-004]
covers: []
depends_on: [SURF.FILT, KRN.LSQ, KRN.MZ]
cli: [fid surf eval-roundness]
mcp: [surf_eval_roundness]
journal: [surf.eval_roundness]
updated: 2026-11-22
---

# SURF.FORM: Precision roundness, harmonic UPR and spindle separation

## Purpose

High-density polar and cylindrical form metrology under ISO 12181 and ISO 12180: harmonic undulations-per-revolution (UPR) Fourier analysis, lobe and bearing-waviness spectra, eccentricity removal, interrupted/notched arc handling and multi-step/Donaldson spindle error separation.

## Scope

**In:** Polar `Profile@1` traces, LSC/MZC/MIC/MCC reference circles (`ISO 12181-1`), harmonic UPR amplitude/phase spectra, radial velocity/acceleration bands, notched/keyway segment exclusion, Donaldson reversal and multi-step spindle-error separation.
**Out:** General CMM prismatic form checks (`GDT.FORM`); linear profile roughness (`SURF.PROF`).

## Requirements

- **REQ-SURF.FORM-001** The subsystem shall compute harmonic UPR spectra and ISO 12181 reference-circle roundness (`RONt`, `RONp`, `RONv`, `RONq`) deterministically under KNP-1 with explicit UPR cutoff bands.
- **REQ-SURF.FORM-002** The subsystem shall support Donaldson reversal and multi-step spindle-error separation with full provenance of paired orientation traces.

## Children

_None yet: status `open`._
