---
id: PLAT.GPU.FALLBK
title: Software and reduced renderer fallback contract
level: 3
parent: PLAT.GPU
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.GPU-003, REQ-PLAT.GPU-005, REQ-PLAT.GPU-008]
covers: []
depends_on: [PLAT.GPU.DETECT, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- CI jobs that measure the floor (`PLAT.GPU.CI`)

## Requirements

- **REQ-PLAT.GPU.FALLBK-001** When `GpuSelection.fallback` is true, the app shall start and offer every non-device workflow, with only the view features listed for that fallback tier reduced or disabled.
- **REQ-PLAT.GPU.FALLBK-002** The project shall publish, per fallback tier, a machine-readable list of view features that are reduced or disabled and how.
- **REQ-PLAT.GPU.FALLBK-003** While in software mode on the minimum hardware profile, the viewer shall display a 1-million-point cloud at no less than 5 frames per second [unverified: target to be measured].
- **REQ-PLAT.GPU.FALLBK-004** The `fid` CLI and MCP server shall start and run every non-view command on a machine with no GPU and no display.
- **REQ-PLAT.GPU.FALLBK-005** For a fixed project and command sequence, the journal digests of every value of record shall be identical whether the app ran on a hardware backend, reduced mode or software mode (CONSTITUTION C2).
- **REQ-PLAT.GPU.FALLBK-006** While a Windows remote desktop session is active, the app shall either run on the session's GPU path or select a fallback tier, and shall not fail to start [unverified: DX12 over RDP behaviour to be measured].

## Interfaces

- **Offers:** fallback tier definitions and the reduced-feature list (read by VIZ, `PLAT.GPU.NOTICE` and the docs).
- **Consumes:** `GpuSelection` from `PLAT.GPU.DETECT`; VIZ renderer and its capability tiers; `PLAT.HW` minimum profile.

## Decomposition hints

TIERS (fallback tiers and reduced-feature list file; likely leaf in `fid-platform`), HEADLESS (no-GPU CLI/MCP guarantee and its test), INVAR (cross-backend journal-digest invariance test), RDP (remote desktop measurement and policy).

## Open questions

- Bundled CPU rasteriser versus a system software adapter is a VIZ implementation choice (inherited from PLAT.GPU).
- DX12 over RDP needs a measured check [unverified].

## Children

_None yet: status `open`._
