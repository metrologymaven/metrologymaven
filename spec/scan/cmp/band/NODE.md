---
id: SCAN.CMP.BAND
title: Tolerance bands and kernel colour binning
level: 3
parent: SCAN.CMP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-SCAN.CMP-007]
covers: [GOM.SRF.10]
depends_on: [SCAN.CMP.DEV]
side_effect: R1
determinism: D0
cli: [fid compare surface-bin]
mcp: [compare_surface_bin]
journal: [compare.surface_bin@1]
updated: 2026-11-16
---

# SCAN.CMP.BAND: Tolerance bands and kernel colour binning

## Purpose

Assigns every value of a deviation field (or thickness field) to a band from a stated band set and returns per-band counts and area fractions. This is the kernel side of every colour map and tolerance-banded legend: VIZ.OVERLAY draws the bin index this node returns and never computes a bin itself, so what the user sees equals what the kernel decided.

## Scope

**In:**
- `BandSet` type: ordered band boundaries in metres, boundary inclusion rule, below-range and above-range bins, tolerance markers
- Per-value bin index for every `VALID` value; a fixed bin for `NO_MATCH` and `EXCLUDED`
- Per-band count and area fraction (mesh: vertex area weights; cloud: point-count weights)
- Canonical bin output keyed by field digest and band-set digest
- Optional restriction to a selection

**Out** (and which node owns it instead):
- ColorMapSpec, palettes, legend layout and drawing (owned by `VIZ.OVERLAY`)
- Producing the field (owned by `SCAN.CMP.DEV`, `SCAN.CMP.ACT`, `SCAN.THK`)
- Conformance decisions (owned by `UNC`; bands are display and summary, not decisions)

## Requirements

- **REQ-SCAN.CMP.BAND-001** When `compare.surface_bin@1` runs with a field and a band set of boundaries b₀ < b₁ < … < bₙ in metres, the system shall assign each `VALID` value v to bin i where bᵢ₋₁ < v ≤ bᵢ, to the below-range bin when v ≤ b₀, and to the above-range bin when v > bₙ.
- **REQ-SCAN.CMP.BAND-002** Values with status `NO_MATCH` or `EXCLUDED` shall be assigned to a dedicated no-data bin and shall be excluded from the area-fraction denominator.
- **REQ-SCAN.CMP.BAND-003** The result shall report, per bin, the count and the weighted fraction, where a mesh vertex weight is one third of the total area of its incident triangles and a cloud point weight is 1, and the fractions of all data bins shall sum to 1 within 1e-12.
- **REQ-SCAN.CMP.BAND-004** If band boundaries are not strictly increasing or contain a non-finite value, then the command shall fail with a typed error.
- **REQ-SCAN.CMP.BAND-005** The bin array and summary shall be determinism class D0, and their canonical serialisation shall be byte-identical for the same field digest and band-set digest.
- **REQ-SCAN.CMP.BAND-006** Binning a 10⁸-value field shall complete within 5 s on reference hardware [unverified].

## Acceptance criteria

- **AC-1** Given fixture `fixtures/scan/field_ramp` (a field of 10 001 values from −0.5 mm to +0.5 mm in 0.1 µm steps on a cloud), when `fid compare surface-bin --field f --bands -0.0003,-0.0001,0.0001,0.0003 --json` runs, then each boundary value falls in the lower bin (for example −0.0001 m goes to the bin ending at −0.0001), and counts equal the analytic counts exactly.
- **AC-2** Given fixture `fixtures/scan/field_mesh_two_area` (a mesh where half the area has +0.2 mm and half −0.2 mm, but with 3× the vertices on one half), when binning with bands at 0, then each data bin's weighted fraction is 0.5 ± 1e-12 while counts differ.
- **AC-3** Given a field with 100 `NO_MATCH` values, they appear in the no-data bin and data fractions still sum to 1.
- **AC-4** Given bands `0.1,0.1`, the command fails with `scan.cmp.bad_band_set`.
- **AC-5** Given the same field and band set run twice on different thread counts, the canonical outputs are byte-identical, and VIZ.OVERLAY's legend test can compare against them (REQ-VIZ.OVERLAY-002).

## Design

- **Registry command:** `compare.surface_bin@1`, R1 (pure compute, scratch result, cached by (field digest, band-set digest)), D0. Parameters: `field` (any field element with values and statuses: deviation or thickness), `bands` (`BandSet`), `selection?`. Result: `{ bins: compact u8/u16 array, summary: [ { bin, lower, upper, count, fraction } ], no_data_count, band_set_digest }`.
- **Data:** `BandSet { boundaries: [f64], tolerance_lower?, tolerance_upper? }` with canonical digest; VIZ.OVERLAY maps its ColorMapSpec onto a `BandSet` and requests re-binning through this command.
- **Algorithm:** binary search per value over the boundaries; weights accumulated in fixed index order with pairwise summation (KNP-1).
- **Errors:** `scan.cmp.bad_band_set`, `scan.cmp.field_not_found`, `geo.selection.stale`.
- **Determinism:** D0.
- **Performance budget:** REQ-006 [unverified].
- **Crate:** `fid-scan-cmp`.

## Test fixtures

- `fixtures/scan/field_ramp`, `fixtures/scan/field_mesh_two_area`: generated, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-scan-cmp`, implement the R1, D0 registry command `compare.surface_bin@1` (CLI `fid compare surface-bin --field <h> --bands b0,b1,... [--selection <h>] [--json]`, MCP `compare_surface_bin`). Define `BandSet` (strictly increasing finite boundaries in metres, optional tolerance markers, canonical digest). For each VALID value v assign bin i with b(i−1) < v ≤ b(i); below-range when v ≤ b0; above-range when v > bn; NO_MATCH and EXCLUDED go to a no-data bin excluded from fractions. Report per-bin count and weighted fraction (mesh vertex weight = one third of incident triangle area; cloud point weight = 1); data-bin fractions sum to 1 within 1e-12. Fail with `scan.cmp.bad_band_set` on non-increasing or non-finite boundaries. Works on any field element with values and statuses (deviation and thickness).
>
> Constraints: CPU only, KNP-1 (pairwise fixed-order summation, total-order comparisons, no FMA, no std HashMap/HashSet), no panic/unwrap/unsafe, canonical (RFC 8785) serialisation of the summary. The viewer must use these bins verbatim, so outputs must be byte-stable. Tests: ramp boundary semantics, area-weighted fractions, no-data handling, bad band set, cross-thread byte equality.

## Open questions

- VIZ.OVERLAY owns ColorMapSpec; this node assumes the mapping ColorMapSpec → `BandSet` lives in VIZ and that VIZ requests binning through this command (VIZ.OVERLAY open question on who owns kernel-side binning is answered here: SCAN for fields).
- GD&T zone colouring at characteristic level stays with GDT.

## Children

_Leaf: none._
