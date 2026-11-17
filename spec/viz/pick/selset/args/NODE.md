---
id: VIZ.PICK.SELSET.ARGS
title: Picked point pairs and datum features as typed command arguments
level: 4
parent: VIZ.PICK.SELSET
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK.SELSET-001]
covers: []
depends_on: [VIZ.PICK.RESOLVE, VIZ.SHELL.PALETTE, CORE.REG.DEF]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.PICK.SELSET.ARGS: Picked point pairs and datum features as typed command arguments

## Purpose

Lets a user fill command arguments of pick-able schema types by clicking in the viewport: a point (entity ID plus f64 point in metres), a point pair (for example nominal and actual points for a point-pair alignment in ALN) and a datum feature (a feature entity for GDT datum commands). The binder plugs into the generated argument forms of the command palette, resolves each click with `view.pick_point@1`, and writes only entity IDs and CPU-resolved f64 coordinates into the form; the command is then invoked normally and journaled by its own domain.

## Scope

**In:** form-field binders for the schema keywords `pick:point`, `pick:point_pair` and `pick:feature` (declared in the registry schema vocabulary); pick sequencing with on-screen prompts ("pick nominal point", "pick actual point"); preview markers; clearing and re-picking.
**Out:** the commands that take these arguments (ALN, GDT); pick resolution (`VIZ.PICK.RESOLVE`); form generation (`VIZ.SHELL.PALETTE`); schema keywords (`CORE.REG.DEF`).

## Requirements

- **REQ-VIZ.PICK.SELSET.ARGS-001** A bound argument shall contain only entity IDs and f64 points returned by `view.pick_point@1`, never pixel coordinates or render IDs.
- **REQ-VIZ.PICK.SELSET.ARGS-002** A `pick:point_pair` field shall require two picks in a stated order and show which one is pending.
- **REQ-VIZ.PICK.SELSET.ARGS-003** A `pick:feature` field shall accept only picks that resolve to a feature entity and shall otherwise show "not a feature" and keep the field empty.
- **REQ-VIZ.PICK.SELSET.ARGS-004** Binding shall not invoke the target command; only the user's confirm in the form does.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube` and a test command with a `pick:point_pair` argument, when the harness picks two vertices, then the form value holds two entity IDs and f64 points equal to the vertices within 1e-9 m and nothing else.
- **AC-2** Given a `pick:feature` field and a pick on raw mesh, then "not a feature" shows and the field stays empty.
- **AC-3** Given binding completed but not confirmed, then nothing is journaled.

## Design

- **Registry command(s):** none; uses `view.pick_point@1`.
- **Data structures:** `PickedPoint { entity, point_m: [f64; 3] }`; `PointPair { first, second }`; `FeatureRef { entity }`.
- **Algorithm:** field binder state machine per keyword; each viewport click → `view.pick_point@1` → validate kind → write value.
- **Errors:** `NotAFeature`, `NothingPicked` (UI).
- **Determinism:** D0.
- **Performance budget:** n/a.
- **Crate:** `fid-pick` (module `selset::args`).

## Test fixtures

`fixtures/viz/golden-cube` (shared) and a test command declared in the test crate.

## Build prompt

> In crate `fid-pick`, add module `selset::args`: form-field binders for registry schema keywords `pick:point`, `pick:point_pair` (two ordered picks with pending prompt) and `pick:feature` (feature entities only, else "not a feature"), plugged into the palette's generated forms. Resolve every click with `view.pick_point@1` and write only entity IDs and f64 points; never invoke the target command (the form's confirm does). Tests AC-1..AC-3. Rules: picks resolved on the CPU (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- The schema keywords `pick:point`, `pick:point_pair` and `pick:feature` must be added to the registry keyword vocabulary (`CORE.REG.DEF.SCHEMA`) [cross-domain].

## Children

_Leaf: none._
