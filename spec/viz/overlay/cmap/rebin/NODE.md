---
id: VIZ.OVERLAY.CMAP.REBIN
title: Kernel re-binning round trip and updating state
level: 4
parent: VIZ.OVERLAY.CMAP
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.OVERLAY.CMAP-004]
covers: []
depends_on: [VIZ.OVERLAY.CMAP.SPEC, SCAN.CMP.BAND, CORE.JOB.SCHED]
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.OVERLAY.CMAP.REBIN: Kernel re-binning round trip and updating state

## Purpose

Keeps the displayed bins equal to the kernel's. Whenever the active spec digest or the field digest changes, it requests `compare.surface_bin@1` (R1) with the spec's BandSet as a cancellable job, keeps the previous bins on screen with an "updating" badge, and swaps in the new bins only when a result arrives whose (field digest, BandSet digest) pair matches the current one. Stale results are discarded. Results are cached by that pair.

## Scope

**In:** `BinState { Current(bins, key), Updating { shown: Option<bins>, pending: key } }`; request, cancel-on-supersede and swap; cache of the last 8 results per viewport; the updating badge state exposed to the LUT pass and legend.
**Out:** the spec (`VIZ.OVERLAY.CMAP.SPEC`); binning (`SCAN.CMP.BAND`); drawing (`VIZ.OVERLAY.CMAP.LUT`, `VIZ.OVERLAY.LEGEND`).

## Requirements

- **REQ-VIZ.OVERLAY.CMAP.REBIN-001** When the active spec digest or field digest changes, the module shall request `compare.surface_bin@1` with `to_band_set(spec)` and shall cancel any earlier pending request for that viewport.
- **REQ-VIZ.OVERLAY.CMAP.REBIN-002** Until a result keyed by the current (field digest, BandSet digest) arrives, the module shall keep showing the previous bins with state `Updating`, and shall never derive bins any other way.
- **REQ-VIZ.OVERLAY.CMAP.REBIN-003** A result whose key differs from the current key shall be discarded.
- **REQ-VIZ.OVERLAY.CMAP.REBIN-004** If binning fails, then the state shall stay `Updating` with the error shown, and the previous bins shall remain.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/devfield-10k` and two specs changed 10 ms apart, when results arrive in reverse order, then only the second result is shown and the first is discarded.
- **AC-2** Given a slow fake binning job, then the state is `Updating` with the previous bins until it returns.
- **AC-3** (error path) Given a binning failure `scan.cmp.bad_band_set`, then the error is shown with the previous bins.
- **AC-4** Given a spec switched back to an earlier one, then the cached result is shown without a new request.

## Design

- **Registry command(s):** none owned; invokes `compare.surface_bin@1` (R1).
- **Data structures:** `BinKey { field_digest, band_set_digest }`; `BinCache` (LRU by insertion order, 8 entries, `BTreeMap` keyed by `BinKey`).
- **Algorithm:** on change, compute key; cache hit → swap; else submit job, mark updating; on completion compare key.
- **Errors:** passed through from SCAN.
- **Determinism:** D0 for state transitions.
- **Performance budget:** swap < 1 frame after the result [unverified].
- **Crate:** `fid-overlay` (module `cmap::rebin`).

## Test fixtures

`fixtures/viz/devfield-10k` (shared); fake binning job in the test crate.

## Build prompt

> In crate `fid-overlay`, add module `cmap::rebin`: on any change of active spec digest or field digest, request `compare.surface_bin@1` (R1) with `cmap::spec::to_band_set` as a cancellable job (cancel superseded ones), keep showing the previous bins with an `Updating` state, and swap only a result whose (field digest, BandSet digest) equals the current key; discard others; keep an 8-entry cache; on failure stay `Updating` with the error. Never compute bins locally. Tests AC-1..AC-4. Rules: display equals kernel (REQ-VIZ-002, C2); no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable.

## Open questions

- None blocking.

## Children

_Leaf: none._
