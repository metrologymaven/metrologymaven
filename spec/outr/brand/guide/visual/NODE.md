---
id: OUTR.BRAND.GUIDE.VISUAL
title: Visual identity principles and screenshot conventions
level: 4
parent: OUTR.BRAND.GUIDE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND.GUIDE-005]
covers: []
depends_on: [OUTR.BRAND.GUIDE.NAME]
uses_artifacts: [UX.TOKENS, UX.ICONS, VIZ.CAPTURE.SHOT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.GUIDE.VISUAL: Visual identity principles and screenshot conventions

## Purpose

How outreach material looks: which design-system tokens it uses for colour and type, the icon style it borrows from the product's original icon set, layout principles, the logo sizes the asset pack must produce, and the screenshot conventions that allow only the project's own software captured with its own capture command. Outreach shares the product's one design system (REQ-FID-031) instead of inventing a second palette; this node chooses from it and supplies the brand-tier values after D-002.

## Scope

**In:** `brand/guidelines/visual.md`; the brand-tier token proposal file `brand/brand-tier.toml` (values pending D-002); the logo size list used by the asset pack; the screenshot rule and the `shot-check` command that verifies capture provenance of every screenshot in outreach sources.
**Out:** token values, themes and palettes (`UX.TOKENS`); the icon set (`UX.ICONS`); name and logo usage (`OUTR.BRAND.GUIDE.NAME`); the trade-dress list (`OUTR.BRAND.GUIDE.DRESS`); the capture command (`VIZ.CAPTURE.SHOT`).

## Requirements

- **REQ-OUTR.BRAND.GUIDE.VISUAL-001** The visual guidelines shall name, for outreach colour and type, only semantic token IDs from the UX.TOKENS set, and shall define no colour values of their own except the brand-tier proposal.
- **REQ-OUTR.BRAND.GUIDE.VISUAL-002** The brand-tier file shall change only tokens that UX.TOKENS marks as brand-tier, and shall record `status = "proposal"` until the D-002 name is accepted.
- **REQ-OUTR.BRAND.GUIDE.VISUAL-003** The guidelines shall state the logo raster sizes (in pixels) and theme variants, matching the asset pack's variant manifest.
- **REQ-OUTR.BRAND.GUIDE.VISUAL-004** The guidelines shall allow only screenshots of the project's own software, taken with the `capture.take_screenshot@1` command, and shall forbid screenshots of incumbent software.
- **REQ-OUTR.BRAND.GUIDE.VISUAL-005** If a raster image under a `screenshots/` folder in an outreach repository has no sidecar `<file>.capture.json` naming command `capture.take_screenshot@1`, the journal entry hash and the image's SHA-256, or the digest does not match, then `shot-check` shall fail and name the file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/brand/visual/ok/` (two PNG screenshots with matching sidecars), when `fid-outreach shot-check --root ok/ --json` runs, then it exits 0.
- **AC-2** (error path) Given `no-sidecar/`, when the check runs, then it exits 1 with `capture_provenance_missing` naming the PNG.
- **AC-3** (error path) Given `wrong-digest/` (sidecar SHA-256 differs from the image), when the check runs, then it exits 1 with `capture_digest_mismatch`.
- **AC-4** (error path) Given `other-command/` (sidecar names a command other than `capture.take_screenshot@1`), when the check runs, then it exits 1 with `capture_command_invalid`.
- **AC-5** Given `brand/brand-tier.toml` and the UX.TOKENS brand-tier list (committed fixture), when the token test runs, then every key in the brand-tier file is a brand-tier token and `status = "proposal"`.
- **AC-6** Given `brand/guidelines/visual.md` and `brand/assets/logo/variants.toml`, when the size test runs, then the sizes and themes listed in both agree.

## Design

- **Command:** `fid-outreach shot-check --root <dir> [--json]`. Project tooling; nearest class R0.
- **Sidecar:** `{ "command": "capture.take_screenshot@1", "journal_entry_hash": "...", "sha256": "...", "app_version": "..." }`, written by whoever captures the screenshot from the capture command's result.
- **Guideline content (own words):** token roles for text, surface, accent and status (status colours keep their locked meaning and are never used decoratively); type families from UX.TOKENS; icon style from UX.ICONS; layout principles (grid, whitespace, one accent colour per page); logo sizes 32, 64, 128, 256, 512 px in light and dark [proposal]; screenshot rules (own software only, capture command only, no cropping that hides status, sample data only).
- **Errors:** `capture_provenance_missing`, `capture_digest_mismatch`, `capture_command_invalid`, `sidecar_parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `shot_check`; documents in the outreach repository.

## Test fixtures

Small generated PNGs and sidecars under `fixtures/outr/brand/visual/` (`ok`, `no-sidecar`, `wrong-digest`, `other-command`), CC-BY-4.0; a committed brand-tier token list fixture copied from the UX.TOKENS package.

## Build prompt

> In the outreach repository, write `brand/guidelines/visual.md` in your own words, using `{{FAMILY_NAME}}` for the family name: outreach colour and type use semantic token IDs from the design-system token set (no new colour values); status colours keep their meaning and are not decoration; icon style follows the product icon set; layout principles; logo sizes 32, 64, 128, 256, 512 px in light and dark (matching `brand/assets/logo/variants.toml`); screenshots only of the project's own software, captured with `capture.take_screenshot@1`, never of competitor software. Add `brand/brand-tier.toml` with `status = "proposal"` and only brand-tier token keys.
>
> In the Rust crate `fid-outreach`, add `fid-outreach shot-check --root <dir> [--json]`: for every raster image (png, jpg, jpeg, webp) under any `screenshots/` folder, require a sidecar `<file>.capture.json` with `command = "capture.take_screenshot@1"`, `journal_entry_hash`, `sha256` equal to the image's SHA-256, and `app_version`; report `capture_provenance_missing`, `capture_digest_mismatch`, `capture_command_invalid`. Exit 0, 1 findings, 2 unreadable input.
>
> Tests: fixtures under `fixtures/outr/brand/visual/` for ok, no sidecar, wrong digest and wrong command; a test that brand-tier keys are all brand-tier tokens (against a committed token-list fixture) and status is proposal; a test that the logo sizes in the guideline and the variant manifest agree.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; nothing resembling a competitor's visual identity.

## Open questions

- Phase P1a, because UX.TOKENS and UX.ICONS (P1a) must exist before the guidelines can name their tokens.
- UX.TOKENS expects OUTR.BRAND.GUIDE to supply brand-tier values after D-002; this leaf delivers them as a proposal file that UX.TOKENS reads (an artifact, not a code edge).

## Children

_Leaf: none._
