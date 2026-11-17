---
id: OUTR.BRAND.ASSET.PACK
title: Outreach asset pack, fonts and logo variants
level: 4
parent: OUTR.BRAND.ASSET
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND.ASSET-004, REQ-OUTR.BRAND.ASSET-005]
covers: []
depends_on: [OUTR.BRAND.ASSET.ATTR]
uses_artifacts: [ECO.RULES.SPDX]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.ASSET.PACK: Outreach asset pack, fonts and logo variants

## Purpose

The asset pack itself: a fixed folder layout, self-hostable fonts, name-agnostic slide and poster templates, and the list of logo variants that must exist once the project name is chosen under D-002. Until then the pack ships everything except the logo, and a small check shows which variants are still missing.

## Scope

**In:** the folder layout `brand/assets/`; the font choice and font files; slide and poster templates using the family-name placeholder; the logo-variant manifest and the `pack-check` command; `ATTRIBUTION.md` rows for every pack file.
**Out:** the attribution and font-origin check (`OUTR.BRAND.ASSET.ATTR`); colour, type and logo usage rules (`OUTR.BRAND.GUIDE.VISUAL`, `OUTR.BRAND.GUIDE.NAME`); choosing the name (D-002); product UI icons (`VIZ`).

## Requirements

- **REQ-OUTR.BRAND.ASSET.PACK-001** The pack shall use the layout `brand/assets/{logo,icons,illustrations,fonts,templates}/` with a `README.md` stating the purpose of each folder.
- **REQ-OUTR.BRAND.ASSET.PACK-002** Every font in `fonts/` shall carry a licence from the allowlist's `asset_licences` that permits self-hosting and web embedding (for example OFL-1.1), with its licence file next to it.
- **REQ-OUTR.BRAND.ASSET.PACK-003** Templates shall use the family-name placeholder token defined by OUTR.BRAND.GUIDE.NAME and no other project name.
- **REQ-OUTR.BRAND.ASSET.PACK-004** The logo-variant manifest `logo/variants.toml` shall list a vector master and the raster sizes and light and dark variants named in the guidelines, each with an expected file path.
- **REQ-OUTR.BRAND.ASSET.PACK-005** While `name_accepted` in `logo/variants.toml` is false, `pack-check` shall report missing logo variants as warnings; when it is true, it shall report each missing variant as an error.
- **REQ-OUTR.BRAND.ASSET.PACK-006** Every file in the pack shall pass the outreach attribution check.

## Acceptance criteria

- **AC-1** Given the pack in the outreach repository with `name_accepted = false`, when `fid-outreach pack-check --root brand/assets --json` runs, then it exits 0 and lists each missing logo variant as a warning.
- **AC-2** (error path) Given fixture `fixtures/outr/brand/pack/accepted-missing/` (`name_accepted = true`, the 512 px dark variant absent), when the check runs, then it exits 1 with `variant_missing logo/raster/512-dark.png`.
- **AC-3** (error path) Given `bad-layout/` (an extra top-level folder `misc/`), when the check runs, then it exits 1 with `layout_unknown misc`.
- **AC-4** (error path) Given `named-template/` (a template containing the working name instead of the placeholder), when the check runs, then it exits 1 with `template_name_literal`.
- **AC-5** Given the pack, when `fid-outreach asset-check --root brand/assets ...` runs, then it exits 0.

## Design

- **Command:** `fid-outreach pack-check --root <dir> [--literal-names <file>] [--json]`. Project tooling; nearest class R0.
- **Variant manifest:** `name_accepted = false`, `[[variant]] path, kind = "vector" | "raster", size_px?, theme = "light" | "dark"`; the initial size list follows the guidelines (proposal: 32, 64, 128, 256, 512 px [unverified until GUIDE.VISUAL fixes it]).
- **Template check:** scans text inside templates (SVG text, ODP and PPTX XML parts read as zip entries, Markdown) for names in `--literal-names` (default: the working name and its lowercase form) and reports `template_name_literal`.
- **Fonts:** one sans-serif and one monospace family under OFL-1.1 [choice unverified until a maintainer reviews it], stored as WOFF2 plus the licence text.
- **Errors:** `layout_unknown`, `variant_missing`, `template_name_literal`, `manifest_parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `pack_check`; pack files in the outreach repository.

## Test fixtures

Small synthetic packs under `fixtures/outr/brand/pack/` (`accepted-missing`, `bad-layout`, `named-template`), CC-BY-4.0.

## Build prompt

> In the outreach repository, create `brand/assets/` with folders `logo`, `icons`, `illustrations`, `fonts`, `templates` and a `README.md` describing each. Add one sans-serif and one monospace font family licensed OFL-1.1 (WOFF2 files plus licence text) and list every file in `ATTRIBUTION.md` (columns path, origin, source, author, licence). Add a slide template and a poster template that use the family-name placeholder token instead of any project name. Add `logo/variants.toml` with `name_accepted = false` and the variant list (vector master; raster 32, 64, 128, 256, 512 px; light and dark). Do not draw a logo yet.
>
> In the Rust crate `fid-outreach`, add `fid-outreach pack-check --root <dir> [--literal-names <file>] [--json]`: fail with `layout_unknown` for unexpected top-level folders; list missing variants as warnings while `name_accepted` is false and as errors (`variant_missing <path>`) when true; fail with `template_name_literal` when a template contains a literal project name (read ODP and PPTX as zip archives with bounded entry sizes). Exit 0, 1 findings, 2 unreadable input.
>
> Tests with fixtures under `fixtures/outr/brand/pack/` for the accepted-but-missing variant, unknown folder and literal name cases, plus the real pack passing both `pack-check` and `asset-check`.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded zip reading; no network; original work only, nothing that resembles a competitor's visual identity.

## Open questions

- The logo itself waits for D-002 (name not chosen). Whether a name-independent symbol could be drawn earlier is a maintainer choice.

## Children

_Leaf: none._
