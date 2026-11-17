---
id: OUTR.BRAND.ASSET
title: Original outreach assets and attribution
level: 3
parent: OUTR.BRAND
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND-002]
covers: []
depends_on: []
uses_artifacts: [ECO.RULES.SPDX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.ASSET: Original outreach assets and attribution

## Purpose

The asset pack used in outreach material (logo files, icons, illustrations, fonts, slide and poster templates) and the attribution check that proves every asset is original or licensed under an allowed licence. It is separate from the guidelines because assets are files with provenance that can be checked automatically, while the guidelines are rules for people.

## Scope

**In:**
- The outreach asset pack and its folder layout
- `ATTRIBUTION.md` entries for every third-party asset (source, author, licence SPDX ID)
- An attribution check over the outreach repositories
- Font choices that can be self-hosted (no third-party font services)

**Out** (and which node owns it instead):
- The SPDX licence allowlist (owned by `ECO.RULES`, per D-001)
- Usage rules for the assets (owned by `OUTR.BRAND.GUIDE`)
- Product UI icons and assets (owned by `VIZ`)

## Requirements

- **REQ-OUTR.BRAND.ASSET-001** Every asset file in the outreach asset pack shall be either marked as original project work or listed in `ATTRIBUTION.md` with its source, author and SPDX licence ID.
- **REQ-OUTR.BRAND.ASSET-002** If an asset is listed under an SPDX licence that is not on the D-001 allowlist kept by ECO.RULES, then the attribution check shall fail and name the asset.
- **REQ-OUTR.BRAND.ASSET-003** If an image, font or media file in an outreach repository is neither marked original nor listed in `ATTRIBUTION.md`, then the attribution check shall fail and name the file.
- **REQ-OUTR.BRAND.ASSET-004** Every font used in outreach material shall be licensed for self-hosting and web embedding and shall be served from the project's own origin.
- **REQ-OUTR.BRAND.ASSET-005** The asset pack shall include the logo in vector form and in raster sizes listed in the guidelines, in both light and dark variants, once the D-002 name is accepted.

## Interfaces

- **Offers:** the asset pack; the attribution check (run by OUTR.SITE.QA, OUTR.DOCS.QA and content builds).
- **Consumes:** ECO.RULES SPDX allowlist; OUTR.BRAND.GUIDE sizes and variants.

## Decomposition hints

PACK (asset files and layout), ATTR (attribution check), FONT (self-hostable fonts).

## Open questions

- How an asset is "marked original" (sidecar file, folder convention or `ATTRIBUTION.md` section) is for the leaf to fix.
- The allowlist is now specified by ECO.RULES.SPDX (`house-rules/spdx-allowlist.toml`, `asset_licences`). The former code dependency on ECO.RULES is an artifact read and is now `uses_artifacts` (CONSTITUTION 2.1, D-034).
- Resolved in OUTR.BRAND.ASSET.ATTR: an asset is marked original by a row with origin `original` in `ATTRIBUTION.md`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.BRAND.ASSET.ATTR](attr/NODE.md) | Outreach attribution and font-origin check (leaf) | 001 002 003 004 | none |
| [OUTR.BRAND.ASSET.PACK](pack/NODE.md) | Outreach asset pack, fonts and logo variants (leaf) | 004 005 | none |
