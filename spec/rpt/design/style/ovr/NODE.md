---
id: RPT.DESIGN.STYLE.OVR
title: Per-block style overrides
level: 4
parent: RPT.DESIGN.STYLE
kind: feature
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-RPT.DESIGN.STYLE-002, REQ-RPT.DESIGN.STYLE-003]
covers: []
depends_on: [RPT.DESIGN.STYLE.SET, RPT.MODEL.DOC]
side_effect: W2
determinism: D0
cli: [fid report set-block-style]
mcp: [report_set_block_style]
journal: [report.set_block_style@1]
updated: 2026-11-17
---

# RPT.DESIGN.STYLE.OVR: Per-block style overrides

## Purpose

Sometimes one block needs to differ from the report's style set: a larger title, a table without borders, a highlighted note. This leaf stores sparse per-block overrides of style fields and computes the effective style of a block as the report's style set with the block's overrides applied. Overrides cannot weaken outcome presentation: the rule that every outcome keeps a text or symbol still applies.

## Scope

**In:** `report.set_block_style@1` (set or clear override fields for one block); sparse override storage keyed by block ID; `effective_style(block)`; removal of overrides when their block is removed.

**Out:** the style-set model and validation rules (`RPT.DESIGN.STYLE.SET`); block frames (`RPT.DESIGN.PLACE`).

## Requirements

- **REQ-RPT.DESIGN.STYLE.OVR-001** `report.set_block_style@1` shall be one journaled W2 command that sets or clears override fields of one block, and replay shall reproduce the same effective style.
- **REQ-RPT.DESIGN.STYLE.OVR-002** The effective style of a block shall be the report's effective style set with that block's override fields replacing the matching fields, and no other change.
- **REQ-RPT.DESIGN.STYLE.OVR-003** Override values shall pass the same validation as style-set fields, including the outcome text-or-symbol rule, else the command shall fail with `report.style_invalid` and journal nothing.
- **REQ-RPT.DESIGN.STYLE.OVR-004** The command shall never change a payload, binding, decision or formatter choice.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/design/one_page_report` with text blocks B1 and B2, when `fid report set-block-style --block B1 --role body --size 14pt --json` runs, then B1's effective body size is 1400 and B2's is unchanged.
- **AC-2** When `--clear body.size` runs for B1, then B1's effective style equals B2's.
- **AC-3** (error path) An override that empties the NONCONFORMS symbol fails with `report.style_invalid`.
- **AC-4** (replay) Given the journal after AC-2, when replayed exactly, then `IDENTICAL`.

## Design

- **Registry command:** `report.set_block_style@1(report, block: BlockId, set: Vec<StyleFieldPatch>, clear: Vec<StyleFieldPath>) -> EffectiveStyle` (W2, `--json`, `--dry-run`).
- **Data structures:** `overrides: BTreeMap<BlockId, StylePatch>` on the report.
- **Errors:** `report.style_invalid { field }`, `report.not_found`.
- **Determinism:** D0.
- **Performance budget:** under 1 ms [unverified until measured].
- **Crate:** `fid-report` (module `style::ovr`).

## Test fixtures

- `fixtures/rpt/design/one_page_report` (shared).

## Build prompt

> In crate `fid-report`, module `style::ovr`, implement sparse per-block style overrides and `report.set_block_style@1` (W2, one journal event, `--json`, `--dry-run`; CLI `fid report set-block-style`; MCP name derived): set or clear typed style fields for one block, validated with the style-set rules (including the outcome text-or-symbol rule, `report.style_invalid`), stored in a `BTreeMap` keyed by block ID and dropped when the block is removed; `effective_style(block)` = report style set plus that block's overrides. Never change payloads, bindings, decisions or formatter choices. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/design/style/ovr/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
