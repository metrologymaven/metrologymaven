---
id: RPT.DESIGN.STYLE.FONT
title: Font references and bundled font list
level: 4
parent: RPT.DESIGN.STYLE
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.STYLE-004, REQ-RPT.DESIGN.STYLE-006]
covers: []
depends_on: [CORE.STO.CAS]
uses_artifacts: [UX.TOKENS, ECO.RULES.SPDX]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# RPT.DESIGN.STYLE.FONT: Font references and bundled font list

## Purpose

Style sets name fonts; the PDF renderer embeds them. This task leaf defines how a style set references a font (family name plus the SHA-256 of the font file), how a reference is resolved (bundled fonts first, then fonts the user added to the project or user font store), the `report.font_missing` error, and the list of fonts the project bundles. The bundled families are the typefaces that `UX.TOKENS` chooses for print; each must carry a licence on the `ECO.RULES.SPDX` allowlist.

## Scope

**In:** `FontRef { family, style: Regular | Bold | Italic | BoldItalic, digest: Sha256 }`; resolver over the bundled set and the content-addressed store; `bundled-fonts.json` (family, style, file digest, SPDX licence ID, source URL) and its build-time licence check.

**Out:** text shaping, subsetting and embedding (`RPT.OUTPUT.PDF`); choosing typefaces (`UX.TOKENS`); the allowlist itself (`ECO.RULES.SPDX`).

## Requirements

- **REQ-RPT.DESIGN.STYLE.FONT-001** A style set shall reference each font by family name, style and SHA-256 of the font file.
- **REQ-RPT.DESIGN.STYLE.FONT-002** If no bundled or stored font file matches a reference's digest, then resolution shall fail with error `report.font_missing` naming the family and style.
- **REQ-RPT.DESIGN.STYLE.FONT-003** The build shall fail if any entry of `bundled-fonts.json` has an SPDX licence ID that is not on the `ECO.RULES.SPDX` allowlist, or a file whose digest differs from the listed one.
- **REQ-RPT.DESIGN.STYLE.FONT-004** Font files shall be treated as untrusted input: the resolver shall check only digest and size (at most 32 MiB) and never parse or execute font programs.

## Acceptance criteria

- **AC-1** Given the bundled set, resolving each family listed in the `UX.TOKENS` print typography returns its file digest.
- **AC-2** (error path) Given a reference with an unknown digest, resolution fails with `report.font_missing` naming family and style.
- **AC-3** Given a test `bundled-fonts.json` entry with licence `LicenseRef-Proprietary`, the licence check fails.
- **AC-4** Given a 40 MiB file added to the user store, resolution refuses it with `report.font_too_large`.

## Design

- **Registry commands:** none (library used by `RPT.DESIGN.STYLE.SET` and `RPT.OUTPUT.PDF`).
- **API:** `resolve(&FontRef) -> Result<FontBlob, FontError>`; `bundled() -> &[BundledFont]`; build-time `check_bundled(allowlist)`.
- **Errors:** `report.font_missing { family, style }`, `report.font_too_large`.
- **Determinism:** D0.
- **Performance budget:** trivial [unverified until measured].
- **Crate:** `fid-report` (module `style::font`).

## Test fixtures

- The bundled fonts (licence on the allowlist, e.g. SIL OFL 1.1 [unverified until the allowlist is published]) and synthetic test entries.

## Build prompt

> In crate `fid-report`, module `style::font`, implement `FontRef { family, style, digest }`, a resolver over bundled fonts and the content-addressed store (`report.font_missing { family, style }`; refuse files over 32 MiB with `report.font_too_large`; never parse font programs here), and `bundled-fonts.json` with a build-time check that every entry's SPDX ID is on the project licence allowlist file and every file matches its digest. The bundled families are those named by the UX design tokens for print. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/design/style/font/NODE.md`.

## Open questions

- Which open-licence families to bundle follows the `UX.TOKENS` typography choice (REQ-UX.TOKENS-011); this leaf only enforces licence and digest.

## Children

_Leaf: none._
