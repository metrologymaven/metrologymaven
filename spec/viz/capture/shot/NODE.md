---
id: VIZ.CAPTURE.SHOT
title: Screenshot command
level: 3
parent: VIZ.CAPTURE
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.CAPTURE-001, REQ-VIZ.CAPTURE-002]
covers: [GOM.VIZ.07]
depends_on: [VIZ.RENDER.HEADLESS, VIZ.VIEW.STATE, CORE.JRN, CORE.STO]
side_effect: W2
determinism: D1
cli: [fid capture take-screenshot]
mcp: [capture_take_screenshot]
journal: [capture.take_screenshot@1]
updated: 2026-11-16
---

# VIZ.CAPTURE.SHOT: Screenshot command

## Purpose

Screenshots as commands: render a viewport (or a NamedView) at a requested resolution and background, store the PNG in the project's content-addressed store with a manifest binding it to the scene, view and kernel digests, and journal one entry with the image digest and a journal-clock timestamp. The same command serves the GUI button, the CLI, MCP and the control channel.

## Scope

**In:** `capture.take_screenshot@1`, viewport and NamedView targets, resolution, background, optional overlays (legend, labels), the image manifest, policy check, storage in the project blob store.
**Out:** off-screen rendering (`VIZ.RENDER.HEADLESS`); full-window OS capture including panels (`VIZ.CAPTURE.OSCAP`); exporting files to a folder (`VIZ.CAPTURE.EXPORT`); recording sessions and the rest of the recording policy (`VIZ.CAPTURE.REC`, P1b).

## Requirements

- **REQ-VIZ.CAPTURE.SHOT-001** `capture.take_screenshot@1` shall render the target viewport or NamedView headlessly at the requested size (16 to 8192 px per side) and background (theme, solid colour or transparent).
- **REQ-VIZ.CAPTURE.SHOT-002** The image shall be stored as PNG in the project content-addressed store, and the journal entry shall carry its SHA-256 digest, the manifest digest and a timestamp from the journal's monotonic clock.
- **REQ-VIZ.CAPTURE.SHOT-003** The manifest shall bind the image digest to the scene digest, ViewState digest, ColorMapSpec and legend digests if shown, kernel manifest digest, tier and adapter.
- **REQ-VIZ.CAPTURE.SHOT-004** If org policy forbids screenshots, then the command shall fail with `PolicyForbidden` and journal nothing.
- **REQ-VIZ.CAPTURE.SHOT-005** The same command shall be reachable from GUI, CLI and MCP, with agent calls committing to the agent branch.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/viz/golden-cube`, when `fid capture take-screenshot --view main --width 800 --height 600 --background solid:#ffffff --json` runs, then the result returns an image digest, the stored PNG's SHA-256 equals it, and the journal holds one `capture.take_screenshot@1` entry with that digest and a monotonic timestamp.
- **AC-2** Given the result of AC-1, then the manifest's scene and view digests equal those of the inputs and the image matches the golden within its tolerance mask.
- **AC-3** (replay) Given the journal from AC-1 on the same adapter, when replayed in exact mode, then the outcome is `IDENTICAL` (same image digest).
- **AC-4** (error path) Given policy `screenshots: forbidden`, when the command runs, then it fails with `PolicyForbidden` and journals nothing.
- **AC-5** Given an MCP agent call, then the entry lands on the agent's branch, not on `main`.

## Design

- **Registry command(s):** `capture.take_screenshot@1`, W2. Parameters: `view: ViewportId | NamedViewId`, `width: u32`, `height: u32`, `background: Theme | Solid(rgb) | Transparent`, `overlays: { legend: bool, labels: bool }`. Result: `{ image_digest, manifest_digest, blob_ref, timestamp }`.
- **Data structures:** `ScreenshotManifest`.
- **Algorithm:** policy check; resolve ViewState; call the headless renderer; encode PNG with fixed encoder settings (no timestamps in metadata); store by digest; journal.
- **Errors:** `PolicyForbidden`, `UnknownView`, `SizeExceedsAdapterLimit`, `StorageFull`.
- **Determinism:** D1 (byte-identical on the same adapter and driver; across GPUs the image is an illustration bound by its manifest).
- **Performance budget:** 1920×1080 screenshot under 1 s on the reference profile [unverified].
- **Crate:** `fid-capture` (module `shot`).

## Test fixtures

`fixtures/viz/golden-cube` (shared), plus a policy fixture forbidding screenshots; in-repo.

## Build prompt

> Create crate `fid-capture` with module `shot` and registry command `capture.take_screenshot@1` (W2; CLI `fid capture take-screenshot`, MCP `capture_take_screenshot`). Check the org-policy key `capture.screenshots` in CORE settings first (`PolicyForbidden`, nothing journaled). Resolve the target viewport or NamedView ViewState, render headlessly with `fid-render::headless` at 16–8192 px with the chosen background and overlays, encode PNG with deterministic settings, store it in the project's content-addressed store, and journal one entry with image digest, manifest digest and the journal's monotonic timestamp. The manifest binds the image to scene, view, colour-map, legend and kernel manifest digests, tier and adapter. Agent calls commit to the agent branch. Tests: digest and journal entry; manifest digests and golden match; exact replay `IDENTICAL` on the same adapter; policy refusal; agent-branch routing. Rules: no panics or `unsafe`; deny unwrap/expect/panic/todo/unimplemented/unreachable; images are illustrations, never values of record.

## Open questions

- This leaf defines the org-policy key `capture.screenshots` (`allowed` or `forbidden`, read from CORE settings) because screenshots ship in P1a; VIZ.CAPTURE.REC (P1b) adds the recording keys to the same policy record.
- `capture.screenshots` is already declared in `fid_capture::rec::policy::capture_settings` (admin, machine scope only, default `allowed`; VIZ.CAPTURE.REC.POLICY, PR #589). This leaf must not declare it again; it checks the policy with `RecordingPolicy::check(MediaKind::Screenshot)`, which fails closed (`Forbid { cause: Unknown }`) on an unreadable or invalid machine value.

## Children

_Leaf: none._
