---
id: PLAT.HW.REFMACH
title: Reference benchmark machine registry
level: 3
parent: PLAT.HW
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-002]
covers: []
depends_on: [PLAT.HW.PROFILE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.HW.REFMACH: Reference machines

## Purpose

Names the physical machines that embody the hardware profiles and on which release benchmarks run, and publishes their full specification, so every published number can be traced to a concrete machine. It also owns the rules for adding, replacing and retiring a reference machine without breaking result comparability.

## Scope

**In:**
- A machine-readable registry of reference machines (`platform/ref-machines.toml`): name, profile it represents, CPU, RAM, GPU, VRAM, storage, display, OS and version, firmware and driver versions, host (who runs it)
- Validation that each machine meets its profile and that the minimum and recommended profiles each have at least one machine
- Rules for replacing a machine and marking a comparability break
- Operating requirements for hosts (no other workloads during benchmark runs, pinned OS updates)

**Out** (and which node owns it instead):
- Who hosts and pays for the machines (D-014)
- Running the suite on them (`PLAT.HW.RUN`)
- The profiles themselves (`PLAT.HW.PROFILE`)

## Requirements

- **REQ-PLAT.HW.REFMACH-001** The project shall publish a registry naming at least one reference machine for the `minimum` profile and one for the `recommended` profile, each with CPU model, core count, RAM, GPU model, VRAM, storage, display, OS and version, and GPU driver version.
- **REQ-PLAT.HW.REFMACH-002** If a registered machine does not meet the profile it claims to represent, then the registry validation shall fail naming the shortfall.
- **REQ-PLAT.HW.REFMACH-003** When a reference machine is replaced or its OS or driver version changes, the registry shall record the change with a date, and the next published results shall mark the comparison with the previous release as a comparability break.
- **REQ-PLAT.HW.REFMACH-004** While a benchmark run is in progress, the reference machine shall run no other scheduled workload [unverified: enforcement mechanism depends on D-014 hosting].

## Interfaces

- **Offers:** `platform/ref-machines.toml` (read by `PLAT.HW.RUN` and the docs).
- **Consumes:** `PLAT.HW.PROFILE`; `PLAT.DOCTOR.LOOKUP` profile check (reused to validate machines); D-014.

## Decomposition hints

REGFILE (registry schema and validator in `fid-platform`; leaf once D-014 names hosts), OPS (host operating rules and comparability-break procedure).

## Open questions

- Blocked on **D-014** (who hosts and pays for the machines). The registry format can be built before it.

## Children

_None yet: status `open`._
