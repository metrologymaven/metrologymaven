---
id: PLAT.GPU
title: GPU backend support and rendering fallbacks
level: 2
parent: PLAT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT-003]
covers: []
depends_on: [PLAT.MATRIX, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.GPU: GPU backends and fallbacks

## Purpose

Defines which GPU backends the desktop apps must run on for each supported platform (Vulkan, Metal, DX12), the order in which backends are tried, and what happens when no supported GPU is present: a software or reduced renderer with a visible notice. It covers the hard environments of the shop floor: virtual machines, remote desktop sessions and old integrated GPUs. It is the platform contract; VIZ implements the renderer against it.

## Scope

**In:**
- Backend-per-platform policy: Vulkan, Metal and DX12 on desktop, and the preferred backend order for each OS
- Minimum GPU feature level and driver version per backend
- Detection of an unsupported or absent GPU and selection of the fallback
- The software or reduced renderer fallback for VMs, remote desktop and headless machines, and its documented limits
- The user-visible notice when running on a fallback
- A known-bad driver list and its workarounds
- Platform CI requirements for rendering on each backend, including a software-renderer job

**Out** (and which node owns it instead):
- Renderer internals, shaders and the CPU reference comparison (`VIZ`)
- WebGPU and WebGL in browsers (`PLAT.WEB`)
- Hardware performance profiles and benchmarks (`PLAT.HW`)
- Reporting the selected backend to the user on demand (`PLAT.DOCTOR`)

## Requirements

- **REQ-PLAT.GPU-001** The project shall publish, for each Tier 1 and Tier 2 platform, the GPU backends supported and the order in which they are tried.
- **REQ-PLAT.GPU-002** The desktop apps shall run on Vulkan on Windows and Linux, on Metal on macOS, and on DX12 on Windows.
- **REQ-PLAT.GPU-003** If no supported hardware GPU backend can be initialised, then the app shall start on a software or reduced renderer rather than failing to start.
- **REQ-PLAT.GPU-004** While the app runs on a fallback renderer, the app shall show a persistent visible notice naming the fallback and linking to the hardware guidance.
- **REQ-PLAT.GPU-005** When running on the software renderer, the app shall display a 1-million-point cloud at no less than 5 frames per second on the minimum hardware profile [unverified: target to be measured].
- **REQ-PLAT.GPU-006** If the detected GPU driver is on the published known-bad list, then the app shall apply the listed workaround or fallback and say so in the notice.
- **REQ-PLAT.GPU-007** When a release is built, CI shall run the rendering test suite on each Tier 1 platform's primary backend and on the software renderer.
- **REQ-PLAT.GPU-008** The choice of backend or fallback shall not change any value of record, per CONSTITUTION C2.

## Interfaces

- **Offers:** backend policy and known-bad driver list (data read by VIZ and `PLAT.DOCTOR`); backend-detection result type (backend, adapter name, driver version, fallback flag, reason).
- **Consumes:** VIZ renderer and its backend selection hook; `PLAT.MATRIX` platform list; `PLAT.HW` minimum profile.

## Decomposition hints

POLICY (per-platform backend order and minimums), DETECT (adapter probing and selection), FALLBK (software or reduced renderer and its limits), NOTICE (user-visible fallback notice), DRVLIST (known-bad drivers), CI (rendering jobs per backend).

## Open questions

- Whether the software fallback is a CPU rasteriser shipped with the app or a system-provided software adapter is a VIZ implementation choice; this node only constrains behaviour.
- Remote-desktop behaviour on Windows (DX12 over RDP) needs a measured check [unverified].

## Children

_None yet: status `open`._
