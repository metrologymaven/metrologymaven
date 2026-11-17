---
id: GDT.DIM.SIZEASME.ENVLP
title: Actual mating envelope size and Rule #1 clearance
level: 4
parent: GDT.DIM.SIZEASME
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.DIM.SIZEASME-002, REQ-GDT.DIM.SIZEASME-003, REQ-GDT.DIM.SIZEASME-004]
covers: [PW.GDT.01]
depends_on: [KRN.ENV, KRN.NUM, GDT.RES.RESOLVE]
uses_artifacts: [UNC.ORS.SIZE]
side_effect: R1
determinism: D3
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DIM.SIZEASME.ENVLP: Actual mating envelope size and Rule #1 clearance

## Purpose

Runs the envelope fit the resolved chain names (for example maximum inscribed cylinder for an internal feature, minimum circumscribed for an external one), reports the envelope size, and compares it with the MMC size from the characteristic's limits: the signed clearance to the MMC boundary of perfect form and whether the surface lies within it. Which fit forms the envelope is the row's choice; this leaf only executes it and does the exact comparison. It also publishes the envelope size as the actual size that `GDT.ZONE.MATL` uses for bonus.

## Scope

**In:** calling the chain-named `KRN.ENV` fit (circle, sphere, cylinder; width features where KRN defines an envelope); envelope size in metres; MMC size from the authored limit (exact decimal); signed clearance (internal: envelope − MMC; external: MMC − envelope; positive means within the boundary); within-boundary boolean by exact `kdec` comparison; `local_optimum_only` and trust-region flags passed through; `EnvelopeResult`.
**Out:** the fits (owned by `KRN.ENV`); local sizes (owned by `GDT.DIM.SIZEASME.LOCAL`); the command and payload (owned by `GDT.DIM.SIZEASME.CMD`); which fit to use and Rule #1 exceptions (owned by DMT rows in `GDT.RES.ROWASME`); bonus (owned by `GDT.ZONE.MATL`).

## Requirements

- **REQ-GDT.DIM.SIZEASME.ENVLP-001** The function shall call exactly the KRN envelope fit the resolved chain names and report its size in metres together with the fit's algorithm ID.
- **REQ-GDT.DIM.SIZEASME.ENVLP-002** The function shall report the signed clearance to the MMC size (positive inside the boundary, by internal or external feature kind) and a within-boundary boolean decided by exact comparison of the f64 envelope size with the authored decimal MMC limit.
- **REQ-GDT.DIM.SIZEASME.ENVLP-003** If the KRN fit carries `local_optimum_only`, then the result shall carry that flag and the field `envelope_scope: local`; otherwise `envelope_scope: global` only when KRN certifies the optimum.
- **REQ-GDT.DIM.SIZEASME.ENVLP-004** If KRN returns `method_undefined` or a trust-region boundary error for the named fit, then the function shall return `UNSUPPORTED` or `envelope_failed{reason}` and no size.
- **REQ-GDT.DIM.SIZEASME.ENVLP-005** Envelope sizes shall match every ORS-SIZE ASME envelope case within its stated tolerance, including sparse and partial-arc cases where an unconstrained inscribed fit escapes.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/size/hole_lobed.json` (internal cylinder, 3-lobe form, known maximum inscribed diameter 9.990 mm, MMC 10.000 mm), when the function runs with a chain naming the MIC cylinder fit, then size = 9.990 mm within the KRN fit's certified tolerance, clearance = −0.010 mm and within-boundary is false.
- **AC-2** Given fixture `pin_round.json` (external cylinder, known MCC 9.995 mm, MMC 10.000 mm), then clearance = +0.005 mm and within-boundary is true.
- **AC-3** Given an envelope size exactly equal to the MMC decimal, then within-boundary is true and clearance is 0.
- **AC-4** Given a KRN stub flagging `local_optimum_only`, then the result carries the flag and `envelope_scope: local`.
- **AC-5** (error path) Given partial-arc fixture `hole_quarter_arc.json` where the trust region is hit, then `envelope_failed{reason: trust_region_boundary}` and no size.
- **AC-6** (ORS) Every ORS-SIZE ASME envelope case passes within its tolerance (runs when `UNC.ORS.SIZE` publishes the suite).

## Design

- **Registry command(s):** none (kernel function `envelope(points, feature: Internal | External, mmc: Decimal, chain, krn) -> Result<EnvelopeResult, EnvError>`).
- **Data structures:** `EnvelopeResult { size_m, fit_alg, clearance_m, within_boundary, envelope_scope, flags }`.
- **Algorithm:** dispatch on the chain's association operator to the KRN envelope command; clearance by subtraction in f64; boolean by `kdec` exact ordering of the f64 against the decimal limit.
- **Errors:** `UNSUPPORTED{alg}`, `envelope_failed{reason}`, `nan_input`.
- **Determinism:** D3, inherited from the iterative KRN envelope fits (D0 for the comparison itself).
- **Performance budget:** dominated by the KRN fit; overhead < 1 µs [unverified].
- **Crate:** `fid-gps` (module `dim::size::envelope`).

## Test fixtures

Seeded synthetic lobed holes and pins with known envelopes (CC0, in-repo); ORS-SIZE suite `ors-size@<semver>` in `ors-case/v1` format from `UNC.ORS.SIZE`.

## Build prompt

> In crate `fid-gps` (kernel), module `dim::size::envelope`, implement `envelope(points, feature, mmc_limit, chain, krn) -> Result<EnvelopeResult, EnvError>`. Call exactly the KRN envelope fit the chain's association operator names (MIC or MCC circle, sphere or cylinder; width envelopes only where KRN defines them, otherwise `UNSUPPORTED`). Report size in metres with the fit's algorithm ID; signed clearance to the MMC limit (internal: size − MMC; external: MMC − size; positive inside); `within_boundary` by exact `kdec` comparison of the f64 size with the authored decimal limit; `envelope_scope: global` only when KRN certifies the optimum, `local` with `local_optimum_only` otherwise; trust-region failure gives `envelope_failed{reason}`. Do not choose a fit yourself. Tests per acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, `fid-num` transcendentals, fixed-order reductions, no std `HashMap`/`HashSet`, total-order min/max, NaN is an error).

## Open questions

- Rule #1 exceptions (for example independency or stock sizes) are row content; when a row says the envelope does not apply, `GDT.DIM.SIZEASME.CMD` simply does not call this function.

## Children

_Leaf: none._
