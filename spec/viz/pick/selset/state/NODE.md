---
id: VIZ.PICK.SELSET.STATE
title: Current selection, set-selection command and algebra orchestration
level: 4
parent: VIZ.PICK.SELSET
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.PICK.SELSET-001, REQ-VIZ.PICK.SELSET-002, REQ-VIZ.PICK.SELSET-003, REQ-VIZ.PICK.SELSET-004, REQ-VIZ.PICK.SELSET-005]
covers: []
depends_on: [VIZ.PICK.RESOLVE, GEO.SEL.CMD, GEO.SEL.QUERY.CMD, CORE.JRN.ENTRY, CORE.REG.DEF]
uses_artifacts: []
side_effect: R1
determinism: D0
cli: [fid view set-selection]
mcp: [view_set_selection]
journal: [view.set_selection@1]
updated: 2026-11-17
---

# VIZ.PICK.SELSET.STATE: Current selection, set-selection command and algebra orchestration

## Purpose

The session's current selection: an ordered set of items, each a whole entity ID or a reference to a GEO selection element (an index set with its digests or a semantic query). `view.set_selection@1` is the one command that changes it from the GUI, CLI and MCP. It is class **R1**: it changes no document state and produces a scratch selection context with a digest; it reaches the Activity Log while recording (D-031), and any W-class command that acts on the current selection records that digest as a consumed `r1_result` input (`CORE.JRN.ENTRY`). Index-level algebra and grow or shrink are not done here: the GUI first runs `geo.combine_selection@1` or `geo.grow_selection@1` (W2, journaled, with their criteria) and then sets the resulting selection element as current.

## Scope

**In:** `CurrentSelection { items: [Entity(EntityId) | Selection(SelectionId)], digest }`; `view.set_selection@1` with operations `replace`, `add`, `subtract`, `intersect`, `clear` at item level; GUI gestures (click, shift-click, ctrl-click, lasso commit, grow, invert) mapped to GEO commands plus `view.set_selection@1`; change notification consumed by `VIZ.PICK.AGENTSEL`.
**Out:** index-set algebra and grow or shrink (`GEO.SEL.ALGEBRA` through `GEO.SEL.CMD`); semantic query evaluation (`GEO.SEL.QUERY.CMD`); named sets (`VIZ.PICK.SELSET.NAMED`); typed pick arguments (`VIZ.PICK.SELSET.ARGS`); agent read (`VIZ.PICK.AGENTSEL`).

## Requirements

- **REQ-VIZ.PICK.SELSET.STATE-001** The current selection shall contain only entity IDs and references to GEO selection elements (index sets with digests or semantic queries), never screen coordinates, render IDs or GPU data.
- **REQ-VIZ.PICK.SELSET.STATE-002** Item-level operations shall be deterministic: the result shall be sorted by (entity ID, selection ID) without duplicates, and the same inputs shall give the same digest.
- **REQ-VIZ.PICK.SELSET.STATE-003** Grow and shrink from the GUI shall be performed by `geo.grow_selection@1` with its ring or radius criterion recorded in that command's entry, and the result shall become current only through `view.set_selection@1`.
- **REQ-VIZ.PICK.SELSET.STATE-004** `view.set_selection@1` shall be class R1, shall change no document state, and shall return the new selection digest, which a W-class command acting on the current selection shall record as an `r1_result` input digest.
- **REQ-VIZ.PICK.SELSET.STATE-005** Every change of the current selection from GUI, CLI or MCP shall go through `view.set_selection@1`.
- **REQ-VIZ.PICK.SELSET.STATE-006** When a journal is replayed in re-targeted mode, selections referenced by semantic query shall be re-evaluated by `geo.reevaluate_selection@1`, and the current selection shall refer to the re-evaluated elements.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/pick/selset` (a cloud and a mesh with two GEO selections), when `fid view set-selection --op add --items sel-1,mesh-2 --json` and then `--op subtract --items mesh-2` run, then the digests equal the fixture's expected values and the items are sorted.
- **AC-2** Given the project database and Record Journal before and after AC-1, then both are unchanged; with recording on, the Activity Log holds two lines.
- **AC-3** Given a GUI grow by 2 rings, then the journal holds one `geo.grow_selection@1` entry with `rings: 2` and the current selection refers to its result.
- **AC-4** Given a W2 test command run on the current selection, then its entry's input digests include the selection digest tagged `r1_result`.
- **AC-5** (replay) Given a session journal using a semantic selection, replayed re-targeted on a modified cloud, then the selection is re-evaluated and its new index digest matches the GEO reference.
- **AC-6** (error path) Given an unknown selection ID, then `UnknownItem` and the selection is unchanged.

## Design

- **Registry command(s):** `view.set_selection@1`, R1, D0; params `{ op: Replace|Add|Subtract|Intersect|Clear, items: [Entity(EntityId) | Selection(SelectionId)] }` (≤ 100,000 items); result `{ items, digest }`. Notification `view.selection_changed { digest }`.
- **Data structures:** `CurrentSelection` per session, canonical JSON digest.
- **Algorithm:** item-level set operations as sorted merges; GUI gestures call GEO commands first when index-level work is needed.
- **Errors:** `UnknownItem`, `TooManyItems`, `StaleSelection` (from GEO, item rejected).
- **Determinism:** D0.
- **Performance budget:** < 5 ms for 10,000 items [unverified].
- **Crate:** `fid-pick` (module `selset::state`).

## Test fixtures

`fixtures/viz/pick/selset`: generated project; CC-BY-4.0.

## Build prompt

> In crate `fid-pick`, add module `selset::state` with registry command `view.set_selection@1` (R1; CLI `fid view set-selection`, MCP `view_set_selection`): a session-scoped current selection of whole entity IDs and GEO selection element references, changed by item-level `replace|add|subtract|intersect|clear`, sorted and deduplicated, with a canonical digest and a `view.selection_changed` notification. It changes no document state. Map GUI gestures to GEO commands first (`geo.combine_selection@1`, `geo.grow_selection@1`, `geo.resolve_selection@1`) when index-level work is needed, then set the result current. Expose the digest so W-class commands on the current selection record it as an `r1_result` input. Tests AC-1..AC-6. Rules: only entity IDs, index sets and queries (C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Settles the side-effect question as R1 (session scratch state, consumed by digest per D-031); a maintainer should confirm this reading of the R1 class.
- Grow by curvature or normal angle is not offered: `GEO.SEL.ALGEBRA` supports rings and radius only; adding those criteria is GEO's [cross-domain].

## Children

_Leaf: none._
