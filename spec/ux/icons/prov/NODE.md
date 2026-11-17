---
id: UX.ICONS.PROV
title: Icon provenance records and icon-set checks
level: 3
parent: UX.ICONS
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.ICONS-002, REQ-UX.ICONS-007, REQ-UX.ICONS-008]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.SYSTEM.REVIEW]
uses_artifacts: [CORE.REG.MAN]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.ICONS.PROV: Icon provenance records and icon-set checks

## Purpose

The provenance trail for every icon and GD&T drawing (author, date, licence, originality declaration, third-party attribution) and the checks that tie the icon set to its users: every labelless icon has an accessible name, and every icon a registry command names exists in the set.

## Scope

**In:** the icon provenance record (an `originality@1` declaration from UX.SYSTEM.REVIEW plus `licence` and `third_party`); `fid-design icons-prov`; the registry icon-name check `fid-design icons-registry`; the ATTRIBUTION.md cross-check.
**Out:** the declaration format and review gate (`UX.SYSTEM.REVIEW`); the icons (`UX.ICONS.CORESET`, `UX.ICONS.CLASS`, `UX.ICONS.GDT`); the registry manifest (CORE.REG.MAN).

## Requirements

- **REQ-UX.ICONS.PROV-001** Every SVG under `icons/` and `gdt/` shall have a provenance record with author, date, licence SPDX ID, originality declaration and `third_party` (false, or the source and licence).
- **REQ-UX.ICONS.PROV-002** If a record has `third_party` set and the source is not listed in the given `ATTRIBUTION.md`, then `fid-design icons-prov` shall fail and name the icon.
- **REQ-UX.ICONS.PROV-003** If an index entry marked `labelless` has no `a11y_id`, then `icons-prov` shall fail and name the icon.
- **REQ-UX.ICONS.PROV-004** When `fid-design icons-registry` runs against a registry manifest, it shall report every command ID whose icon name is not in the icon index, with the command ID and the missing name.

## Acceptance criteria

- **AC-1** Given the shipped icons and `ATTRIBUTION.md`, when `fid-design icons-prov --root design --attribution ATTRIBUTION.md --json` runs, then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/ux/icons/prov/no-record/`, when the check runs, then it exits 1 with `PROV-MISSING <icon>`.
- **AC-3** (error path) Given `third-party-unlisted/`, when the check runs, then it exits 1 with `PROV-ATTRIBUTION <icon>`.
- **AC-4** (error path) Given manifest fixture `fixtures/ux/icons/prov/manifest-icons.json` where `align.run_bestfit@1` names icon `align.bestfit-x`, when `fid-design icons-registry --manifest manifest-icons.json --root design --json` runs, then it exits 1 with `ICON-REGISTRY-MISSING align.run_bestfit@1 align.bestfit-x`.

## Design

- **Commands:** `fid-design icons-prov --root <dir> --attribution <file> [--json]`; `fid-design icons-registry --manifest <file> --root <dir> [--json]`. Tooling, nearest class R0.
- **Manifest read:** the per-command `icon` field if present; commands without the field are listed as `no_icon_field` (informational) until CORE.REG adds it.
- **Errors:** `PROV-MISSING`, `PROV-SCHEMA`, `PROV-ATTRIBUTION`, `PROV-A11Y-MISSING`, `ICON-REGISTRY-MISSING`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `icons_prov`.

## Test fixtures

`fixtures/ux/icons/prov/`: `no-record/`, `third-party-unlisted/`, `manifest-icons.json` (synthetic manifest with an `icon` field); CC-BY-4.0.

## Build prompt

> In the Rust tooling crate `fid-design`, add `fid-design icons-prov --root <dir> --attribution <file> [--json]` (every SVG in `icons/` and `gdt/` has a record with author, date, SPDX licence, originality declaration, third_party; third-party sources appear in ATTRIBUTION.md; labelless icons have `a11y_id`) and `fid-design icons-registry --manifest <file> --root <dir> [--json]` (every command icon name in the registry manifest exists in the icon index; commands without an icon field are reported as informational `no_icon_field`). Errors `PROV-MISSING|PROV-SCHEMA|PROV-ATTRIBUTION|PROV-A11Y-MISSING|ICON-REGISTRY-MISSING`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.ICONS.PROV.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; manifest at most 32 MiB, bounded JSON; no network.

## Open questions

- CORE.REG.MAN's manifest has no per-command `icon` field today; REQ-UX.ICONS-008 needs CORE.REG to add one (cross-domain, not editable from here).

## Children

_Leaf: none._
