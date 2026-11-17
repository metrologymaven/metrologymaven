---
id: VIZ.RENDER.RECOVER
title: GPU device-loss recovery
level: 3
parent: VIZ.RENDER
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.RENDER-008]
covers: []
depends_on: [VIZ.RENDER.SCENE, VIZ.RENDER.FRAME]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# VIZ.RENDER.RECOVER: GPU device-loss recovery

## Purpose

Survive GPU device loss (driver resets, common on shop-floor integrated GPUs) without losing committed work: drop every GPU resource, acquire a new device, and rebuild from the CPU-side scene graph, stepping down a capability tier if recovery keeps failing. Recovery never touches the command bus or journal.

## Scope

**In:** loss detection, resource teardown and rebuild, retry and tier step-down policy, recovery counters, the "display unavailable" state.
**Out:** scene graph content (`VIZ.RENDER.SCENE`); tier classification (`VIZ.RENDER.FRAME`); crash-restart of the whole app (`CORE`).

## Requirements

- **REQ-VIZ.RENDER.RECOVER-001** When wgpu reports device loss, the renderer shall drop all GPU resources, request a new device and rebuild all buffers from the scene graph.
- **REQ-VIZ.RENDER.RECOVER-002** Recovery shall not invoke any registry command and shall not append to the journal.
- **REQ-VIZ.RENDER.RECOVER-003** Recovery shall present a new frame within 5 s of the loss for the benchmark scene on the reference profile [unverified].
- **REQ-VIZ.RENDER.RECOVER-004** If three recoveries fail within 60 s, then the renderer shall step down one capability tier and retry, and after the lowest tier fails it shall show a persistent "display unavailable" state while the engine keeps running.
- **REQ-VIZ.RENDER.RECOVER-005** Each recovery attempt and its outcome shall be counted and exposed through the render-info data.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/bench-small` loaded and the journal head digest recorded, when the test hook `inject_device_loss()` fires, then a new frame is presented, the scene node count is unchanged, and the journal head digest is unchanged.
- **AC-2** Given the same setup, when recovery completes, then the off-screen image of the stored view matches the pre-loss image within the golden tolerance mask.
- **AC-3** Given a test adapter that fails device creation three times, when loss is injected, then the tier reported afterwards is one lower than before and the recovery counter shows 3 failures and 1 success.
- **AC-4** (error path) Given every tier failing, when loss is injected, then the renderer reports state `DisplayUnavailable`, no panic occurs, and a registry command issued afterwards still commits.
- **AC-5** Given AC-1 on the software adapter, then time from injection to the next presented frame is recorded in the test output (budget checked on the reference profile only).

## Design

- **Registry command(s):** none; counters surface through `view.get_render_info@1` (owned by `VIZ.RENDER.FRAME`).
- **Data structures:** `RecoveryState { Healthy, Recovering { attempt }, SteppedDown { from, to }, DisplayUnavailable }`; `RecoveryCounters { attempts, successes, failures, last_ms }`.
- **Algorithm:** register the device-lost callback; on loss, mark state Recovering, drop resource caches, recreate adapter and device on the current tier, replay uploads from the scene graph's CPU copies (bulk data comes from memory-mapped blobs, so nothing is lost), resume. Track failures in a 60 s window; on the third, step down one tier.
- **Errors:** `AdapterUnavailable`, `RebuildFailed { stage }`; never propagated as a panic.
- **Determinism:** N (timing-dependent); the rebuilt image is compared by tolerance.
- **Performance budget:** see REQ-003 [unverified].
- **Crate:** `fid-render` (module `recover`).

## Test fixtures

`fixtures/viz/bench-small`: generated mixed scene (1 M points, 100 k triangles, seed 7), in-repo. A test-only adapter wrapper injects device loss and creation failures.

## Build prompt

> In crate `fid-render`, add module `recover`. Detect device loss via the wgpu device-lost callback; then drop every GPU resource, recreate the adapter and device on the current capability tier, and rebuild all buffers from the CPU-side `SceneGraph`. Never call the command bus or write the journal. After three failed attempts within 60 s step down one tier; after the lowest tier fails enter `DisplayUnavailable` while the app keeps running. Expose `RecoveryCounters` for render info. Add a test-only `inject_device_loss()` hook and an adapter wrapper that can fail creation N times. Tests: journal head and scene node count unchanged after recovery; post-recovery image matches pre-loss within the golden tolerance mask; tier step-down after three failures; total failure yields `DisplayUnavailable` with no panic and commands still commit. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- Whether a later physical UI/engine process split changes the recovery contract is left to CORE.

## Children

_Leaf: none._
