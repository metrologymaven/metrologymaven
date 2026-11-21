---
id: VIZ.RENDER.SCENE.STALE
title: Stale marking and digest check
level: 4
parent: VIZ.RENDER.SCENE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-render]
satisfies: [REQ-VIZ.RENDER.SCENE-004]
covers: []
depends_on: [VIZ.RENDER.SCENE.PROJ, CORE.DEP.PROP]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# VIZ.RENDER.SCENE.STALE: Stale marking and digest check

## Purpose

Show a result as stale from the moment an upstream input changes until recompute delivers a new source digest, and make sure a stale node can never be styled as conforming or nonconforming. This is the display side of CORE's `outdated` marking.

## Scope

**In:** the `DisplayStatus` state machine for result nodes; mapping CORE `outdated` marks to `Stale`; clearing on a new source digest; the style guard.
**Out:** computing which elements are outdated (`CORE.DEP.PROP`); the status glyphs and colours (`VIZ.OVERLAY.STATUS`); projection (`VIZ.RENDER.SCENE.PROJ`).

## Requirements

- **REQ-VIZ.RENDER.SCENE.STALE-001** When CORE marks an element `outdated`, the scene graph shall set every node bound to it, and every derived node of it, to `Stale` in the same change batch.
- **REQ-VIZ.RENDER.SCENE.STALE-002** A `Stale` node shall leave `Stale` only when the projector delivers a data digest different from the digest recorded when it became stale, or the same digest with CORE's `outdated` mark cleared.
- **REQ-VIZ.RENDER.SCENE.STALE-003** While a node is `Stale`, the style resolver shall never return a conforming or nonconforming style for it, and shall return the stale style supplied by `VIZ.OVERLAY.STATUS`.
- **REQ-VIZ.RENDER.SCENE.STALE-004** The scene graph shall expose the count of stale nodes after every change batch.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/scene-doc-small` with an alignment changed, when CORE publishes the outdated set, then all 12 characteristic nodes and their labels are `Stale` in that batch.
- **AC-2** Given recompute completes with new digests, then the nodes return to their kernel-supplied statuses and the stale count is 0.
- **AC-3** (error path) Given a stale node whose kernel status is `CONFORMS`, when the style resolver runs, then it returns the stale style, never the conforming style.
- **AC-4** Given recompute that yields the same digest but clears `outdated`, then the node leaves `Stale`.

## Design

- **Registry command(s):** none; library.
- **Data structures:** `DisplayStatus = Kernel(Outcome) | Stale { since_digest } | NotEvaluated | Error(code)`; `StyleResolver` wrapper that consults `DisplayStatus` before kernel outcome.
- **Algorithm:** subscribe to CORE outdated marks; mark nodes and derived nodes; on change batch compare digests; resolver checks `Stale` first.
- **Errors:** none beyond propagation.
- **Determinism:** D0.
- **Performance budget:** marking 10^4 nodes under 2 ms [unverified].
- **Crate:** `fid-render` (module `scene::stale`).

## Test fixtures

`fixtures/viz/scene-doc-small` (shared with PROJ).

## Build prompt

> In crate `fid-render`, add module `scene::stale`. Define `DisplayStatus { Kernel(Outcome), Stale { since_digest }, NotEvaluated, Error(code) }`. When CORE marks an element outdated, set its nodes and derived nodes `Stale` in the same change batch; leave `Stale` only on a new data digest or when the outdated mark clears. Wrap the style resolver so a stale node always gets the stale style from the overlay status mapping and never a conforming or nonconforming style. Expose the stale count. Tests AC-1..AC-4 with `fixtures/viz/scene-doc-small`. Rules: CONSTITUTION §8; C2 (statuses come from the kernel, never computed here).

## Open questions

- None.

## As built

- `fid-render` module `scene::stale` (PR to follow in `built_pr`). `DisplayStatus::Stale` carries `since_digest` and the `evidence` facts (claimed, declared input, uncharacterized input) of the status it replaced, so a claimed or provisional value never looks measured while stale (D-045). Serialised `stale:<flags>:<since digest>`. A document may not supply `Stale` (`StaleInDocument`).
- REQ-002 is built strictly: a stale node leaves `Stale` only when the projector delivers it again **and** CORE's mark on its element is cleared (`clear_outdated`, or `recomputed` for both in one batch). A new digest while the mark is still set keeps the node stale with the new data. This implies REQ-002's condition and never shows a result CORE still calls outdated as current. A node added for a marked element starts stale.
- REQ-003: `StyleResolver::new` refuses status styles where a stale style equals any conforming or nonconforming style, a stale value with evidence facts looks like a stale measured one, or a conforming or nonconforming value with evidence facts looks measured. The stale styles are read once, at construction.
- `fid-overlay` `from_display_status` maps `Stale` to `DisplayState::Stale` with its evidence (VIZ.OVERLAY.STATUS AC-12, tested there).

## Children

_Leaf: none._
