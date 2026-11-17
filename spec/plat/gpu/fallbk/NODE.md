---
id: PLAT.GPU.FALLBK
title: Software and reduced renderer fallback contract
level: 3
parent: PLAT.GPU
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-003, REQ-PLAT.GPU-005, REQ-PLAT.GPU-008]
covers: []
depends_on: [PLAT.GPU.DETECT]
uses_artifacts: [VIZ.RENDER.FRAME, VIZ.RENDER.HEADLESS, PLAT.HW.SCHEMA, PLAT.HW.REFMACH, ECO.DATA.EX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.GPU.FALLBK: Fallback renderer contract

## Purpose

Defines what "software or reduced renderer" means for virtual machines, remote desktop sessions, headless machines and GPUs below the minimum: which renderer is used, which view features are reduced or disabled, the performance floor, and the guarantee that nothing about values of record changes. VIZ implements it; this node fixes the behaviour and the limits that users and IT are told about.

## Scope

**In:**
- The fallback tiers: reduced hardware mode (old or known-bad GPU with features off) and software mode (no usable GPU)
- The list of view features disabled or degraded in each fallback tier, published as data
- The performance floor in software mode on the minimum hardware profile
- Headless operation: CLI and MCP never need a GPU
- The C2 invariance guarantee across backends

**Out** (and which node owns it instead):
- Renderer implementation and the choice between a bundled CPU rasteriser and a system software adapter (`VIZ`)
- Backend selection (`PLAT.GPU.DETECT`)
- The notice (`PLAT.GPU.NOTICE`)
- CI jobs that run these tests per release (`PLAT.GPU.CI`)

## Requirements

- **REQ-PLAT.GPU.FALLBK-001** When `GpuSelection.fallback` is true, the app shall start and offer every non-device workflow, with only the view features listed for that fallback tier reduced or disabled.
- **REQ-PLAT.GPU.FALLBK-002** The project shall publish, per fallback tier, a machine-readable list of view features that are reduced or disabled and how.
- **REQ-PLAT.GPU.FALLBK-003** While in software mode on the minimum hardware profile, the viewer shall display a 1-million-point cloud at no less than 5 frames per second [unverified: target to be measured].
- **REQ-PLAT.GPU.FALLBK-004** The `fid` CLI and MCP server shall start and run every non-view command on a machine with no GPU and no display.
- **REQ-PLAT.GPU.FALLBK-005** For a fixed project and command sequence, the journal digests of every value of record shall be identical whether the app ran on a hardware backend, reduced mode or software mode (CONSTITUTION C2).
- **REQ-PLAT.GPU.FALLBK-006** While a Windows remote desktop session is active, the app shall either run on the session's GPU path or select a fallback tier, and shall not fail to start [unverified: DX12 over RDP behaviour to be measured].

## Interfaces

- **Offers:** `platform/gpu-fallback.toml` (tiers, reduced features, software floor; read by VIZ, `PLAT.GPU.NOTICE` and docs); the floor check; the headless and forced-fallback smoke tests; the invariance harness `xtask plat-gpu invariance` (run by `PLAT.GPU.CI`); the RDP start test.
- **Consumes:** `GpuSelection` (`PLAT.GPU.DETECT`). Artifacts: VIZ capability tiers and `view.get_render_info@1` output (`VIZ.RENDER.FRAME`), headless rendering (`VIZ.RENDER.HEADLESS`), benchmark results (`PLAT.HW.SCHEMA`) of the minimum-profile reference machine (`PLAT.HW.REFMACH`), and the example project (`ECO.DATA.EX`).

## Decomposition hints

See Children.

## Open questions

- Bundled CPU rasteriser versus a system software adapter is a VIZ implementation choice (inherited from PLAT.GPU).
- DX12 over RDP needs a measured check [unverified]; carried by `PLAT.GPU.FALLBK.RDP`.

## Split rationale

Split by **kind of guarantee**: a data file and its floor check (TIERS), start-up guarantees without a GPU (SMOKE), the value-of-record invariance harness (INVAR), and the remote-desktop case (RDP), which needs its own environment. Each is tested differently and has a different owner of failures.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.GPU.FALLBK.TIERS | Fallback tier file, reduced-feature list and software floor check | 002 003 | none |
| PLAT.GPU.FALLBK.SMOKE | No-GPU headless and forced-fallback start tests | 001 004 | none |
| PLAT.GPU.FALLBK.INVAR | Cross-backend value-of-record invariance harness | 005 | none |
| PLAT.GPU.FALLBK.RDP | Windows remote desktop start test | 001 006 | none |
