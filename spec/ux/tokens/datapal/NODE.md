---
id: UX.TOKENS.DATAPAL
title: Deviation palette catalogue
level: 3
parent: UX.TOKENS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.TOKENS-006, REQ-UX.TOKENS-007]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.TOKENS.DATAPAL: Deviation palette catalogue

## Purpose

The catalogue of data palettes used for deviation colour maps in the viewer and in report snapshots: stepped tolerance, colour-vision-deficiency-safe stepped (the default), and perceptually uniform continuous maps, each with a stable ID and version. The palettes are values; binning and the ColorMapSpec entity belong to VIZ.OVERLAY.CMAP and the kernel.

## Scope

**In:** `tokens/palettes/catalogue.json` (`palette-catalogue@1`) and one file per palette; IDs and versioning rules; the default-palette marker; palette-level structural checks.
**Out:** colour-difference and colour-vision verification (`UX.TOKENS.CHECK`); status colours (`UX.TOKENS.STATUS`); ColorMapSpec, binning and LUTs (`VIZ.OVERLAY.CMAP`); report legend style (`UX.REPORT.CHART`).

## Requirements

- **REQ-UX.TOKENS.DATAPAL-001** The catalogue shall contain at least `pal.step.tol@1` (stepped tolerance), `pal.step.cvd@1` (CVD-safe stepped), `pal.cont.viridis@1` and `pal.cont.cividis@1`, and shall mark `pal.step.cvd@1` as the default deviation palette.
- **REQ-UX.TOKENS.DATAPAL-002** Each stepped palette shall be diverging with an odd number of steps between 7 and 15, a neutral centre step for in-tolerance-near-nominal, and separate under-range and over-range colours.
- **REQ-UX.TOKENS.DATAPAL-003** Each continuous palette shall be given as at least 256 sRGB samples with its source and licence recorded, and only palettes whose source licence is public domain or CC0 [unverified for viridis and cividis] shall be included.
- **REQ-UX.TOKENS.DATAPAL-004** A palette's colours shall never change within a palette ID and major version; any colour change shall create a new major version, and the old version shall remain in the catalogue.
- **REQ-UX.TOKENS.DATAPAL-005** The stepped palettes shall be designed to meet REQ-UX.TOKENS-007 (monotonic lightness per arm; adjacent-step ΔE2000 at least 10 under normal and simulated vision), as reported by `UX.TOKENS.CHECK`.

## Acceptance criteria

- **AC-1** Given the shipped catalogue, when `fid-design palettes-check --root design --json` runs, then it exits 0 and lists the four required IDs with `pal.step.cvd@1` as default.
- **AC-2** (error path) Given fixture `fixtures/ux/tokens/datapal/even-steps/` (an 8-step stepped palette), when the check runs, then it exits 1 with `PAL-STEPS`.
- **AC-3** (error path) Given `changed-v1/` (a colour of `pal.step.tol@1` differs from the committed baseline digest), when the check runs with `--baseline`, then it exits 1 with `PAL-IMMUTABLE pal.step.tol@1`.
- **AC-4** Given the shipped stepped palettes, when `UX.TOKENS.CHECK` runs, then every stepped palette passes the lightness and ΔE rules.

## Design

- **Command:** `fid-design palettes-check --root <dir> [--baseline <catalogue digests>] [--json]`. Tooling, nearest class R0.
- **Palette file:** `{ id, version, kind: stepped|continuous, colors: ["#rrggbb"], under, over, centre_index?, source, licence, digest }`; digest is SHA-256 of the RFC 8785 form of `colors`, `under`, `over`.
- **Design method:** stepped palettes built in a perceptual space (e.g. CAM02-UCS or OKLab) from blue to orange arms rather than red/green, so that the main CVD types keep the arms apart [unverified that this meets the ΔE thresholds until checked].
- **Errors:** `PAL-STEPS`, `PAL-CENTRE`, `PAL-SAMPLES`, `PAL-LICENCE`, `PAL-IMMUTABLE`, `PAL-DEFAULT`, `PAL-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `palettes`.

## Test fixtures

`fixtures/ux/tokens/datapal/`: `even-steps/`, `changed-v1/` with a baseline digest file; synthetic, CC-BY-4.0. Viridis and cividis samples come from their published sources with licence recorded.

## Build prompt

> Author `tokens/palettes/` in the design-system package: `catalogue.json` (`palette-catalogue@1`) listing `pal.step.tol@1`, `pal.step.cvd@1` (default), `pal.cont.viridis@1`, `pal.cont.cividis@1`; one file per palette `{id, version, kind, colors, under, over, centre_index, source, licence, digest}`. Stepped palettes: odd step count 7..15, neutral centre, diverging blue/orange arms designed in a perceptual space for colour-vision safety, original colours. Continuous palettes: 256 sRGB samples from their public-domain or CC0 sources (verify the licence before including).
>
> In the Rust tooling crate `fid-design`, add `fid-design palettes-check --root <dir> [--baseline <file>] [--json]` checking structure, step count, centre, sample count, licence field, default marker and immutability against a baseline of digests. Errors `PAL-STEPS|PAL-CENTRE|PAL-SAMPLES|PAL-LICENCE|PAL-IMMUTABLE|PAL-DEFAULT|PAL-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-3 of spec node UX.TOKENS.DATAPAL.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; RFC 8785 digests via the project canonical-JSON crate; no network.

## Open questions

- VIZ.OVERLAY.CMAP lists the same four palettes by name; it should resolve them by the IDs here (`uses_artifacts: [UX.TOKENS.DATAPAL]`, not editable from here).

## Children

_Leaf: none._
