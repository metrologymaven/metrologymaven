---
id: PLAT.GPU.NOTICE
title: Fallback and workaround notice
level: 3
parent: PLAT.GPU
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-004, REQ-PLAT.GPU-006]
covers: []
depends_on: [PLAT.GPU.DETECT]
uses_artifacts: [PLAT.GPU.FALLBK.TIERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-gpu-select]
updated: 2026-11-22
---

# PLAT.GPU.NOTICE: Fallback and workaround notice

## Purpose

Makes a degraded rendering state visible. Whenever the app runs on a fallback tier or with a known-bad-driver workaround, the user sees a persistent notice that names the state, says what is reduced and links to the hardware guidance. This leaf is the pure content builder and the display contract; VIZ draws it.

## Scope

**In:**
- `build_notice(selection, fallback_tiers) -> Option<RenderNotice>`, a pure function
- The `RenderNotice` type with display rules VIZ must honour (`collapsible: true`, `dismissible: false`)
- English message text with stable message IDs; the guidance link `docs/hardware.html#fallback` (offline docs path)

**Out** (and which node owns it instead):
- Drawing the notice in the shell status area (`VIZ.SHELL.DOCK`)
- Fallback tiers and reduced features (`PLAT.GPU.FALLBK.TIERS`)
- On-demand reporting (`PLAT.DOCTOR`)
- Hardware guidance page content (`PLAT.HW` and `OUTR.DOCS`)

## Requirements

- **REQ-PLAT.GPU.NOTICE-001** When `GpuSelection.fallback` is true, `build_notice` shall return a notice naming the fallback tier, the selection reason, the reduced or disabled features of that tier, and the guidance link.
- **REQ-PLAT.GPU.NOTICE-002** When the selection lists applied known-bad entries or workarounds, the notice shall name each workaround and known-bad entry ID, even if `fallback` is false.
- **REQ-PLAT.GPU.NOTICE-003** Every notice shall carry `collapsible = true` and `dismissible = false`, and the display contract shall require VIZ to keep a collapsed indicator visible for the whole session while the notice exists.
- **REQ-PLAT.GPU.NOTICE-004** When neither a fallback nor a workaround applies, `build_notice` shall return `None`.
- **REQ-PLAT.GPU.NOTICE-005** `build_notice` shall be pure: the same inputs give a byte-identical serialised notice, and adapter-supplied strings (adapter name, driver version) shall be included only as escaped untrusted text (CONSTITUTION C6).

## Acceptance criteria

- **AC-1** Given the `GpuSelection` from `PLAT.GPU.DETECT` AC-2 (software fallback) and `fixtures/plat/gpu/fallback-valid.toml`, when `build_notice` runs, then the serialised notice equals `fixtures/plat/gpu/notice-software.json` and lists the software tier's disabled features.
- **AC-2** Given DETECT AC-3's selection (vulkan with an `avoid_backend` entry), then the notice names the entry ID and `fallback` is not mentioned as a tier.
- **AC-3** Given DETECT AC-1's selection, then `None`.
- **AC-4** Given an adapter name containing an ANSI escape and a newline, then the notice text contains neither control byte.
- **AC-5** (determinism) Given AC-1 twice, then byte-identical output.

## Design

- **Registry command(s):** none; library used by VIZ. The notice state is visible to agents through `view.get_render_info@1` (VIZ), which should include the notice ID (cross-domain note below).
- **Data structures:** `RenderNotice { id: "gpu.fallback.software" | "gpu.fallback.reduced" | "gpu.workaround", tier: Option<TierName>, reason: String, features: Vec<FeatureEffect>, entries: Vec<KnownBadId>, workarounds: Vec<String>, link: "docs/hardware.html#fallback", collapsible: true, dismissible: false, text: { title, body } }`.
- **Errors:** none; an unknown tier in the selection yields a generic notice with tier `unknown`.
- **Determinism:** D0.
- **Crate:** `fid-gpu-select` (module `notice`).

## Test fixtures

Selections from `PLAT.GPU.DETECT` fixtures, fallback file from `PLAT.GPU.FALLBK.TIERS`, expected notices under `fixtures/plat/gpu/`. Licensed with the code.

## Build prompt

> In crate `fid-gpu-select`, add module `notice` with pure `build_notice(selection: &GpuSelection, tiers: &GpuFallback) -> Option<RenderNotice>`. Return a notice when `fallback` is true (tier, reason, that tier's reduced/disabled features from `platform/gpu-fallback.toml`) or when known-bad entries or workarounds were applied (entry IDs, workaround names); otherwise `None`. Every notice has `collapsible: true`, `dismissible: false`, a stable message ID, English title and body, and link `docs/hardware.html#fallback`. Escape adapter-supplied strings (strip control characters). Serialise with a fixed field order. Document the display contract for VIZ (collapsed indicator always visible while the notice exists). Tests: AC-1..AC-5. Constraints: CONSTITUTION C2 (display only), C6, §8. Commit: `feat(fid-gpu-select): render notice builder`.

## Open questions

- Drawing the notice belongs to `VIZ.SHELL.DOCK` (status bar), which does not yet list it; VIZ should also include the notice ID in `view.get_render_info@1` (cross-domain).
- Localisation follows VIZ's scheme; launch-1 is English only.

## Children

_Leaf: none._
