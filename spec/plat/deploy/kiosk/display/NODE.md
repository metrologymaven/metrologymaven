---
id: PLAT.DEPLOY.KIOSK.DISPLAY
title: Reference kiosk display file
level: 4
parent: PLAT.DEPLOY.KIOSK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY.KIOSK-003]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.KIOSK.DISPLAY: Reference kiosk display

## Purpose

Names the display that kiosk layouts are designed and tested against, as one small machine-readable file, so VIZ.KIOSK's size rules (touch-target size, minimum font size) cite concrete numbers instead of guesses.

## Scope

**In:**
- `platform/kiosk-display.toml`: diagonal, native resolution, pixel density, touch technology, glove support, a generic description (no brand needed)
- Schema, parser, validator and derived values (physical pixels per millimetre)

**Out** (and which node owns it instead):
- Kiosk layout and touch-target rules (`VIZ.KIOSK`)
- Guides (`PLAT.DEPLOY.KIOSK.GUIDE`)

## Requirements

- **REQ-PLAT.DEPLOY.KIOSK.DISPLAY-001** The file shall state `diagonal_in`, `resolution = [w, h]`, `touch` (`projected_capacitive`, `resistive` or `none`), `glove_touch` (bool) and `description`, with `schema_version` 1.
- **REQ-PLAT.DEPLOY.KIOSK.DISPLAY-002** The crate shall derive `px_per_mm` from diagonal and resolution assuming square pixels, rounded to 3 decimals.
- **REQ-PLAT.DEPLOY.KIOSK.DISPLAY-003** If `diagonal_in` is outside 10–43, a resolution side is outside 800–7680, or a field is unknown, then validation shall fail naming the field.
- **REQ-PLAT.DEPLOY.KIOSK.DISPLAY-004** The shipped file shall describe a 21.5-inch 1920×1080 projected-capacitive glove-capable panel [unverified: proposal until a maintainer confirms a typical shop-floor model].

## Acceptance criteria

- **AC-1** Given the shipped file, when `cargo xtask plat-kiosk display` runs, then it exits 0 and prints `px_per_mm = 4.040` [unverified arithmetic to be confirmed by the test].
- **AC-2** (error path) Given `fixtures/plat/kiosk/display-tiny.toml` with `diagonal_in = 5`, when validated, then it exits 1 with `OutOfRange { field: diagonal_in }`.
- **AC-3** (error path) Given `fixtures/plat/kiosk/display-extra.toml` with field `brightness`, then `UnknownField`.

## Design

- **Registry command(s):** none; data file plus `xtask plat-kiosk display`.
- **Data structures:** `KioskDisplay { schema_version, diagonal_in: f64, resolution: (u32, u32), touch: Touch, glove_touch: bool, description: String }`.
- **Algorithm:** `px_per_mm = hypot(w, h) / (diagonal_in × 25.4)`.
- **Errors:** `Parse`, `UnknownField`, `OutOfRange`, `TooLarge` (cap 16 KiB).
- **Determinism:** D0.
- **Crate:** `fid-platform` (module `kiosk_display`).

## Test fixtures

Hand-written TOML under `fixtures/plat/kiosk/`; licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `kiosk_display` and `xtask plat-kiosk display [--file <path>]`: parse `platform/kiosk-display.toml` (cap 16 KiB, `deny_unknown_fields`, `schema_version` 1) with `diagonal_in` (10–43), `resolution` ([w, h], each 800–7680), `touch` (projected_capacitive | resistive | none), `glove_touch`, `description`; print derived `px_per_mm = hypot(w,h) / (diagonal_in*25.4)` to 3 decimals. Ship the file with a 21.5-inch 1920×1080 projected-capacitive glove-capable panel marked provisional in a comment. Tests: AC-1..AC-3. Constraints: CONSTITUTION §8. Commit: `feat(fid-platform): reference kiosk display`.

## Open questions

- The concrete panel is a maintainer choice; the values are a proposal.

## Children

_Leaf: none._
