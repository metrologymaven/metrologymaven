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
built_in: [fid-design]
satisfies: [REQ-UX.TOKENS-006, REQ-UX.TOKENS-007]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- As built: `catalogue.json` is `{ format: "palette-catalogue@1", default: "<ref>", palettes: ["<ref>", ..] }` and each palette lives in `tokens/palettes/<ref>.json` (`<ref>` = `<id>@<major>`); any other file in that directory is `PAL-SCHEMA`. The baseline is `{ format: "palette-digests@1", digests: { "<ref>": "sha256:.." } }`; `fixtures/ux/tokens/datapal/shipped-digests.json` holds the shipped digests.
- As built: the shipped palettes are original, written by `fixtures/ux/tokens/datapal/make_palettes.py` from OKLCH paths. `pal.cont.viridis@1` and `pal.cont.cividis@1` keep the IDs of REQ-001 but are not the published viridis or cividis data; they are this project's maps built to the same published design goals (strictly rising lightness; cividis on the blue-yellow axis), and their `source` says so. Being project-made data they are `CC-BY-4.0` (LICENSE.md). `PAL-LICENCE` accepts `CC0-1.0` or `public-domain` for a palette from elsewhere (REQ-003) and `CC-BY-4.0` only when the source starts `original: `.
- As built: perceptual rules are gated in `palettes-check` (as `STATUS-DISTANCE` is in `status-check`); `UX.TOKENS.CHECK` should repeat them as the release gate. Codes beyond the Design list: `PAL-LIGHTNESS` (strict CIELAB L* monotonicity per arm, and along a continuous map), `PAL-DELTA <ref> <a> <b>` (CIEDE2000 at least 10 under normal vision and Machado 2009 protanopia, deuteranopia, tritanopia for adjacent steps, mirrored steps `c-k`/`c+k` so the arms stay apart, and `under`/`over` against the end colours), `PAL-OOT`, `PAL-CONTRAST` and `PAL-CLAIMED` (below). `PAL-CENTRE` also fails a centre step with OKLab chroma above 0.02 [unverified threshold].
- As built (closes the out-of-tolerance item UX.TOKENS.STATUS deferred here): every palette's `under` and `over` are the out-of-tolerance region colours. `PAL-OOT` requires CIEDE2000 at least 20 from `sem.status.conforms` in every theme under the four vision conditions (REQ-UX.TOKENS-009); the shipped pair (ochre `#b18e15`, coral `#ea6878`, shared by all four palettes) gives at least 23.28. Because CONFORMS is azure across lightness 29 to 68 (CIELAB L*), no blue, violet, teal or green colour can meet this, so both out-of-tolerance colours are warm even on the blue arm's side. `PAL-CONTRAST` requires them to reach 3:1 on `sem.surface.base` in every theme (WCAG 2.2 SC 1.4.11); shipped 3.11. The contrast of the in-range steps on the surfaces is measured and recorded in `expected.json`, not gated: a diverging map with a light centre cannot reach 3:1 on both a white and a near-black surface, so legend swatches need a border (UX.REPORT.CHART, VIZ.OVERLAY.CMAP).
- As built: UX.TOKENS.COLOR's claimed-versus-measured floor is extended to data colours: `PAL-CLAIMED <ref> <theme>` fails any palette colour within OKLab 0.10 of a claimed semantic role in that theme. Shipped minimum 0.1087.
- As built: shipped margins: least step difference 11.98 (`pal.step.cvd@1`) and 10.80 (`pal.step.tol@1`); continuous maps rise by at least 0.05 L* per sample.
- No categorical palette: the spec names none. Distinguishability under colour-vision deficiency is measured on the stepped palettes (adjacent and mirrored steps).
- AC-4 pending UX.TOKENS.CHECK: until it exists, the lightness and ΔE rules are shown by `palettes-check` and by `tests/tokens_datapal.rs`, matched to the independent oracle `fixtures/ux/tokens/datapal/make_fixtures.py` (28 fixtures, hand-labelled; perceptual labels recomputed by the oracle).

## Children

_Leaf: none._
