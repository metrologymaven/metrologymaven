---
id: PLAT
title: Platform support: OS, hardware, GPU and deployment matrix
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-017, REQ-FID-023]
covers: []
depends_on: [ECO, VIZ]
cli: [fid doctor *]
mcp: [doctor_*]
journal: []
updated: 2026-11-16
---

# PLAT: Platform support

## Purpose

Defines and enforces where the platform runs:
- operating systems and versions;
- CPU architectures;
- GPU backends and minimum or recommended hardware;
- the web (WASM) target;
- shop-floor and air-gapped deployment;
- the constraint that many instrument SDKs are Windows-only.

It also owns `fid doctor`, a self-check command that tells users whether their machine is supported.

## Scope

**In:**
- Support matrix: Windows 10/11, Linux distributions, macOS versions, x86-64 and ARM64
- Support tiers (Tier 1 tested in CI, Tier 2 builds, Tier 3 community)
- GPU backends (Vulkan, Metal, DX12, WebGPU) and fallbacks, including software rendering for VMs and remote desktop
- Minimum and recommended hardware profiles and reference benchmark machines
- Air-gapped and offline install, locked-down shop-floor PCs, non-admin install
- Enterprise deployment (silent MSI, managed configuration)
- Kiosk and touchscreen machines
- Platform constraints of instrument SDKs (Windows-only drivers through out-of-process hosts)
- `fid doctor` environment diagnostics
- Platform CI coverage

**Out:**
- Installer building and signing (ECO.REL)
- Driver implementation (HAL)
- Rendering internals (VIZ)

## Requirements

- **REQ-PLAT-001** The project shall publish a support matrix that assigns every OS, version and architecture combination a tier, and Tier 1 combinations shall be built and tested in CI on every release.
- **REQ-PLAT-002** Windows 10/11 x86-64, current Ubuntu LTS x86-64 and current macOS ARM64 shall be Tier 1 from P1a [unverified until D-005 and CI capacity are confirmed].
- **REQ-PLAT-003** The apps shall run on Vulkan, Metal and DX12, and fall back to a software or reduced renderer when no supported GPU is present, with a visible notice.
- **REQ-PLAT-004** The project shall publish minimum and recommended hardware profiles, with benchmark results on named reference machines for each release.
- **REQ-PLAT-005** The apps shall install and run fully offline (air-gapped), without administrator rights for per-user installs, and with silent managed installs for enterprises.
- **REQ-PLAT-006** When an instrument SDK exists only for one OS, the system shall run that driver in an out-of-process host on a supported machine and reach it over the HAL host protocol, so the apps themselves stay cross-platform.
- **REQ-PLAT-007** `fid doctor` shall report the OS, GPU backend, driver versions, available memory and tier, and say whether the machine meets the minimum profile, in human-readable and JSON form.
- **REQ-PLAT-008** The viewer and the non-device workflows shall run in current browsers through the WASM build, with documented feature differences from desktop.

## Interfaces

- **Offers:** the support matrix, hardware profiles, `fid doctor`, platform CI requirements.
- **Consumes:** ECO (CI, releases), VIZ (renderer backends), HAL (host protocol).

## Decomposition hints

MATRIX (OS and arch tiers), GPU (backends and fallbacks), HW (hardware profiles and reference machines), DEPLOY (offline, non-admin, enterprise, kiosk), XHOST (Windows-only SDK strategy), DOCTOR, WEB (browser support).

## Open questions

- Linux ARM64 and Windows ARM64 tiers: decide when the matrix is expanded.

## Split rationale

The split is by **platform concern**: each child owns one axis of "where does it run" that has its own data, its own CI jobs and its own pace of change. MATRIX is the shared source of truth (OS and architecture tiers); GPU, HW, DEPLOY, XHOST and WEB each add one axis on top of it; DOCTOR reads all of them and reports to the user. Shared data lives in exactly one child and the others `depends_on` it, so no responsibility is owned twice. Platform CI coverage is not a child of its own: each child owns the CI jobs for its axis, and MATRIX owns the release gate.

Alternatives considered:
- **Split by operating system** (Windows / Linux / macOS / Web). Rejected: GPU fallbacks, hardware profiles and deployment rules cut across every OS and would be repeated three times.
- **Split by audience** (end user / IT administrator / CI). Rejected: the same matrix and profile data serve all three, so ownership would blur.
- **A separate CI child.** Rejected: CI requirements are only meaningful next to the axis they test; a CI child would own everyone else's acceptance gates.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.MATRIX | Support matrix: OS and architecture tiers with platform CI | 001 002 | none |
| PLAT.GPU | GPU backend support and rendering fallbacks | 003 | none |
| PLAT.HW | Hardware profiles, reference machines and release benchmarks | 004 | none |
| PLAT.DEPLOY | Shop-floor deployment: offline, non-admin, enterprise and kiosk | 005 | none |
| PLAT.XHOST | Cross-platform strategy for single-OS instrument SDKs | 006 | none |
| PLAT.DOCTOR | fid doctor environment diagnostics | 007 | none |
| PLAT.WEB | Browser support for the WASM build | 008 | none |
