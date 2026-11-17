---
id: VIZ.OVERLAY.CMAP.SPEC
title: ColorMapSpec model and the set-colormap command
level: 4
parent: VIZ.OVERLAY.CMAP
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.CMAP-003, REQ-VIZ.OVERLAY.CMAP-005, REQ-VIZ.OVERLAY.CMAP-006]
covers: []
depends_on: [KRN.NUM.KDEC, CORE.JRN.CANON, CORE.REG.DEF, CORE.PRJ.TREE.MODEL]
uses_artifacts: [UX.TOKENS.DATAPAL]
side_effect: W2
determinism: D0
cli: [fid view set-colormap]
mcp: [view_set_colormap]
journal: [view.set_colormap@1]
updated: 2026-11-17
---

# VIZ.OVERLAY.CMAP.SPEC: ColorMapSpec model and the set-colormap command

## Purpose

The `ColorMapSpec` document entity and the only command that creates or changes it. A spec binds one field element (deviation or thickness) to a mode (stepped or continuous), bin edges stored as exact decimal strings in metres, a display unit, a palette ID and version from the UX palette catalogue, an out-of-range style and optional informative uncertainty bands. Its canonical JSON digest is what screenshots, legends, NamedViews and replay compare. The spec also maps deterministically to the SCAN `BandSet` used for kernel binning.

## Scope

**In:** `ColorMapSpec` type, canonical form (RFC 8785) and digest; validation (strictly increasing edges, 1 to 64 bins, known palette ID and version from `palette-catalogue@1`, step count compatible with a stepped palette, unit of kind length); `view.set_colormap@1` (W2) creating or replacing the spec of a field; `to_band_set(spec) -> BandSet` with each edge converted from decimal to f64 by one correctly rounded conversion (`KRN.NUM.KDEC`).
**Out:** re-binning and the updating state (`VIZ.OVERLAY.CMAP.REBIN`); GPU lookup (`VIZ.OVERLAY.CMAP.LUT`); the palette values (`UX.TOKENS.DATAPAL`); the editor UI (`VIZ.OVERLAY.SCALEED`); binning (`SCAN.CMP.BAND`).

## Requirements

- **REQ-VIZ.OVERLAY.CMAP.SPEC-001** Bin edges shall be stored as exact decimal strings in metres, and if they are not strictly increasing under exact decimal comparison, then `view.set_colormap@1` shall fail with `NonMonotonicEdges` and journal nothing.
- **REQ-VIZ.OVERLAY.CMAP.SPEC-002** If the palette ID and version are not in `palette-catalogue@1`, or a stepped palette's colour count does not equal the spec's data-bin count, then the command shall fail with `UnknownPalette` or `PaletteStepMismatch` and journal nothing.
- **REQ-VIZ.OVERLAY.CMAP.SPEC-003** `view.set_colormap@1` shall be the only operation that creates or changes a ColorMapSpec, and its entry shall carry the spec's SHA-256 digest of the RFC 8785 form.
- **REQ-VIZ.OVERLAY.CMAP.SPEC-004** `to_band_set` shall convert each edge with a single correctly rounded decimal-to-f64 conversion and shall return the same BandSet digest on every platform.
- **REQ-VIZ.OVERLAY.CMAP.SPEC-005** When the journal is replayed in exact mode, each `view.set_colormap@1` entry shall reproduce the recorded spec digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/devfield-10k` and spec file `cmap-5bin.json` (edges `-0.0005, -0.0002, 0.0002, 0.0005` m, palette `pal.step.cvd@1`), when `fid view set-colormap --field dev1 --spec cmap-5bin.json --json` runs, then the result digest equals the fixture's expected digest and one entry is journaled with it.
- **AC-2** (error path) Given edges `0.0002, 0.0002`, then `NonMonotonicEdges` and nothing journaled; given `pal.unknown@1`, then `UnknownPalette`.
- **AC-3** Given `to_band_set(cmap-5bin)` on Linux, Windows and macOS CI, then the BandSet digests are identical.
- **AC-4** (replay) Given the journal of AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `view.set_colormap@1`, W2, D0; params `{ field: ElementRef, spec: ColorMapSpec }`; result `{ spec_id, digest }`.
- **Data structures:** `ColorMapSpec { field, mode: Stepped|Continuous, edges_m: Vec<DecimalString>, display_unit: LengthUnit, palette: { id, version }, out_of_range: ClampHatch|Transparent|Neutral, uncertainty_bands?: Vec<DecimalString> }` registered as an element type.
- **Algorithm:** parse edges with `kdec`; validate; canonicalise; digest; register or replace the element; journal.
- **Errors:** `NonMonotonicEdges`, `TooManyBins`, `UnknownPalette`, `PaletteStepMismatch`, `BadUnit`, `UnknownField`.
- **Determinism:** D0.
- **Performance budget:** < 5 ms per command excluding commit [unverified].
- **Crate:** `fid-overlay` (module `cmap::spec`).

## Test fixtures

`fixtures/viz/devfield-10k` and `cmap-5bin.json` (shared with VIZ.OVERLAY.LEGEND); a copy of `palette-catalogue@1` from the UX design-system package.

## Build prompt

> In crate `fid-overlay`, add module `cmap::spec`: the `ColorMapSpec` element (field ref, mode, edges as exact decimal strings in metres, display unit, palette ID+version from `palette-catalogue@1`, out-of-range style, optional informative uncertainty bands), its RFC 8785 canonical form and SHA-256 digest, and registry command `view.set_colormap@1` (W2; CLI `fid view set-colormap`, MCP `view_set_colormap`) as the only way to create or change it. Validate strictly increasing edges with exact decimal comparison (`fid-num` kdec), 1–64 bins, known palette, stepped colour count = data bins, length unit. Provide `to_band_set(&spec) -> BandSet` with one correctly rounded conversion per edge. Tests AC-1..AC-4. Rules: no value of record computed here (C1); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Characteristic-level zones from GDT are not mapped to a ColorMapSpec here; only SCAN fields are binned (`compare.surface_bin@1`).

## Children

_Leaf: none._
