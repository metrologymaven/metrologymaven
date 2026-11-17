---
id: GDT.LOC.POS.CMD
title: Position command and dual-variant reporting
level: 4
parent: GDT.LOC.POS
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-GDT.LOC.POS-001, REQ-GDT.LOC.POS-002, REQ-GDT.LOC.POS-003]
covers: [GOM.GDT.08, PW.GDT.04]
depends_on: [GDT.LOC.POS.RFS, GDT.LOC.POS.MMC, GDT.LOC.POS.PATTERN, GDT.LOC.DERIV.TYPES, GDT.LOC.LRES, GDT.DRF.BUILD, GDT.DRF.CHECK.FIELDS, GDT.CHAR.SPEC, GDT.CHAR.RECORD, GDT.RES.RESOLVE]
side_effect: W2
determinism: D3
cli: [fid check eval-position]
mcp: [check_eval_position]
journal: [check.eval_position@1]
updated: 2026-11-17
---

# GDT.LOC.POS.CMD: Position command and dual-variant reporting

## Purpose

Owns `check.eval_position@1`: gate, resolve, build the DRF, derive the toleranced elements, and dispatch to single-feature RFS, material-condition or pattern evaluation. When the characteristic asks for both interpretation variants, it evaluates both and marks the variant the row designates as deciding, so a report shows both numbers and which one counts. It fills `LocPayload` and the datum-mobility fields for every member.

## Scope

**In:** `check.eval_position@1` (W2; dry-run R1); dispatch on the characteristic (single or pattern) and resolver output (RFS, MMC, LMC; variant); `--variants both` reporting with `deciding: true` on the row's designated variant; one ResultRecord per characteristic (pattern members as an array inside the payload, each with the group ID); `datum_mobility` and `frame` fields; `evaluation_basis: POINTS`.
**Out:** the evaluations (owned by `GDT.LOC.POS.RFS`, `GDT.LOC.POS.MMC`, `GDT.LOC.POS.PATTERN`); which variant decides (owned by DMT rows); composite tiers (owned by `GDT.ZONE.COMP`); non-cylindrical features (owned by `GDT.LOC.POS.SLOT`).

## Requirements

- **REQ-GDT.LOC.POS.CMD-001** The command shall evaluate the variant selected by the resolver by default, and when both variants are requested it shall evaluate both and mark the row's designated deciding variant with `deciding: true`.
- **REQ-GDT.LOC.POS.CMD-002** If the row designates no deciding variant and both are requested, then the command shall report both with `deciding` absent and set the ResultRecord decision input to the resolver-selected variant only.
- **REQ-GDT.LOC.POS.CMD-003** For a pattern, the command shall emit one ResultRecord for the characteristic whose payload lists every member's `LocPayload` with the group ID.
- **REQ-GDT.LOC.POS.CMD-004** If any step fails, then the command shall journal nothing and emit no ResultRecord.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/loc/pos_hole_offset.json` and a synthetic ASME RFS row, when `fid check eval-position --char C --points P --json` runs, then value = 0.100 mm within 1e-12 m and the payload carries deviations and `datum_mobility`.
- **AC-2** Given an MMC characteristic, a synthetic row with variants `surface` (deciding) and `axis`, and `--variants both`, then both variants are reported and `surface` has `deciding: true`.
- **AC-3** Given `pos_pattern_4x.json`, then one ResultRecord holds four member payloads with one group ID.
- **AC-4** (error path) Given a characteristic with no governing spec, then `governing_spec_missing` and nothing journaled.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.

## Design

- **Registry command(s):** `check.eval_position@1`, W2, dry-run R1; params `char: CharId`, `points: PointSetRef | [PointSetRef]` (pattern), `variants: selected | both`; result ResultRecord with `PositionPayload { variants: [{name, deciding?, members: [LocPayload]}] }`.
- **Data structures:** `PositionPayload`.
- **Algorithm:** gate, resolve, DRF, derive, dispatch, payload, mobility fields, builder.
- **Errors:** pass-through from every step.
- **Determinism:** D3 when fits or solvers iterate; recorded per result.
- **Performance budget:** dominated by fits [unverified].
- **Crate:** `fid-gps` (module `loc::position::cmd`).

## Test fixtures

Fixtures of the sibling leaves; synthetic DMT rows with placeholder variants (CC0).

## Build prompt

> In crate `fid-gps`, module `loc::position::cmd`, implement registry command `check.eval_position@1` (W2, dry-run R1, `--json`; CLI `fid check eval-position`; MCP `check_eval_position`; one journal event). Pipeline: governing-spec gate, resolve, build or fetch the DRF, `derive()` each toleranced element, then dispatch to `position_rfs`, `position_material` or `position_pattern`. With `variants: both`, evaluate both interpretation variants and mark the row's designated deciding variant `deciding: true` (absent if the row designates none; the decision input stays the resolver-selected variant). One ResultRecord per characteristic; pattern members listed in the payload with the group ID; fill `datum_mobility` and `frame` fields; `evaluation_basis: POINTS`. Errors journal nothing. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1, no std `HashMap`/`HashSet`; GDT never decides conformance.

## Open questions

- Surface versus axis precedence is DMT content (D-009, D-016); this leaf only reads the row's designation.

## Children

_Leaf: none._
