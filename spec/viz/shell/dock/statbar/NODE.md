---
id: VIZ.SHELL.DOCK.STATBAR
title: Status bar as provenance strip
level: 4
parent: VIZ.SHELL.DOCK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.DOCK-003]
covers: []
depends_on: [VIZ.SHELL.DOCK.HOST, VIZ.RENDER.FRAME.TIER, CORE.REG.MAN]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.DOCK.STATBAR: Status bar as provenance strip

## Purpose

A status bar that always tells the user what they are looking at: kernel manifest digest, registry manifest digest, journal head, display units, capability tier, displayed versus total primitive counts and recording state. It hosts the recording indicator that `VIZ.CAPTURE.REC` drives, and reads everything from R0 queries or published data; it never computes a value.

## Scope

**In:** the status bar widget; its fields and update rules; truncated digests with full text on hover and in the accessibility name; host slot for the recording indicator.
**Out:** recording behaviour (`VIZ.CAPTURE.REC`); render info (`VIZ.RENDER.FRAME.TIER`); units preference (`I18N.UNITS`).

## Requirements

- **REQ-VIZ.SHELL.DOCK.STATBAR-001** The status bar shall show kernel manifest digest, registry manifest digest, journal head, display units, capability tier, displayed and total points and triangles, and recording state at all times while a project is open.
- **REQ-VIZ.SHELL.DOCK.STATBAR-002** When the LOD budget limits drawing, the counts field shall show displayed and total with a reduced marker, and its accessible name shall state both numbers.
- **REQ-VIZ.SHELL.DOCK.STATBAR-003** Digests shall show the first 12 hex characters and expose the full digest on hover, on copy and in the accessible description.
- **REQ-VIZ.SHELL.DOCK.STATBAR-004** The status bar shall refresh the journal head within one frame of a commit event and the counts at most every 250 ms.
- **REQ-VIZ.SHELL.DOCK.STATBAR-005** The recording slot shall show the indicator state supplied as data by `VIZ.CAPTURE.REC` and shall never be hidden while recording is active.

## Acceptance criteria

- **AC-1** Given the test app with `fixtures/projects/min.fid`, when a command commits, then the next frame's status bar shows the new journal head prefix.
- **AC-2** Given `fixtures/viz/cloud-1m` with budget 10^5, then the counts field shows displayed ≤ 100 000 of 1 000 000 with the reduced marker, and the accessibility tree name contains both numbers.
- **AC-3** Given a recording session active, when the user tries to hide the status bar, then the recording slot remains visible as a floating chip.
- **AC-4** Given copy on the kernel digest, then the clipboard holds the full 64-hex digest.

## Design

- **Registry command(s):** none; reads `view.get_render_info@1`, `registry.show_manifest@1` and the journal head event.
- **Data structures:** `StatusModel { kernel_digest, manifest_digest, head, units, tier, counts, recording: RecIndicator }` (`RecIndicator` is plain data set by the capture crate through the host slot).
- **Errors:** missing source shows "unavailable" text, never stale values.
- **Determinism:** N (UI).
- **Performance budget:** under 0.3 ms per frame.
- **Crate:** `fid-shell` (module `dock::statbar`).

## Test fixtures

`fixtures/projects/min.fid`, `fixtures/viz/cloud-1m`.

## Build prompt

> In crate `fid-shell`, add `dock::statbar`: a status bar showing kernel and registry manifest digests (12-hex prefix; full digest on hover, copy and accessible description), journal head (refreshed within a frame of a commit), display units, capability tier, displayed/total points and triangles (reduced marker when LOD limits; refresh ≤ 4 Hz) and a recording slot whose data is set by the capture crate and stays visible while recording. Sources are R0 queries and events only. Tests AC-1..AC-4. Rules: CONSTITUTION §8; C2.

## Open questions

- None.

## Children

_Leaf: none._
