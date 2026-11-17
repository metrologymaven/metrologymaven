---
id: PLAT.HW.REFMACH
title: Reference benchmark machine registry
level: 3
parent: PLAT.HW
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-PLAT.HW-002]
covers: []
depends_on: [PLAT.HW.PROFILE, PLAT.DOCTOR.LOOKUP]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.HW.REFMACH: Reference machines

## Purpose

Names the machines that embody the hardware profiles and on which release benchmarks run, and publishes their full specification and who hosts them (D-014), so every published number can be traced to a concrete machine. It also owns the change log that marks comparability breaks when a machine is replaced or its OS or driver changes, and the operating rules hosts follow.

## Scope

**In:**
- `platform/ref-machines.toml` schema 1: machines with name, profile, hosting, host (person or provider), CPU, cores, RAM, GPU, VRAM, storage, display, OS and version, GPU driver version, and a dated change log
- Hosting kinds per D-014: `maintainer_owned` (minimum profile) and `cloud_rented` (recommended profile, rented per release with a pinned instance type)
- Validation: each machine meets its claimed profile (reusing the doctor's profile check), and `minimum` and `recommended` each have at least one machine
- `docs/benchmarks/reference-machines.md` generated from the file, with the host of each machine
- `docs/benchmarks/host-rules.md`: operating rules for hosts

**Out** (and which node owns it instead):
- Running the suite and publishing results (`PLAT.HW.RUN`)
- The profiles themselves (`PLAT.HW.PROFILE`)
- Funding and procurement (maintainers, per D-014)

## Requirements

- **REQ-PLAT.HW.REFMACH-001** The registry shall name at least one machine for the `minimum` profile and one for the `recommended` profile, each with CPU model, core count, RAM, GPU model, VRAM, storage, display, OS and version, GPU driver version, hosting kind and host.
- **REQ-PLAT.HW.REFMACH-002** If a registered machine does not meet the profile it claims, as decided by the doctor's profile verdict on a snapshot built from its registry entry, then validation shall fail naming the machine and each shortfall.
- **REQ-PLAT.HW.REFMACH-003** A `cloud_rented` machine shall name its provider instance type, and its entry shall be treated as unchanged across rentals only while the instance type, OS version and driver version are unchanged.
- **REQ-PLAT.HW.REFMACH-004** When a machine's hardware, OS version, driver version or instance type changes, the change log shall gain a dated entry, and `is_comparable(machine, release_a, release_b)` shall return false if any such entry falls between the two releases' dates.
- **REQ-PLAT.HW.REFMACH-005** The host rules page shall require that benchmark runs start only after a 10-minute idle period with no other scheduled workload and with OS updates paused for the run, and that each run record a check of these conditions [unverified: enforcement on rented instances].
- **REQ-PLAT.HW.REFMACH-006** The generated machines page shall name the host of every machine and shall be checked for drift in CI.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/hw/ref-machines-valid.toml` (one maintainer-owned minimum machine, one cloud-rented recommended machine) and `fixtures/plat/hw/profiles-valid.toml`, when `cargo xtask plat-hw validate-machines` runs, then it exits 0.
- **AC-2** (error path) Given `ref-machines-short.toml` whose minimum machine has 8 GiB RAM, then it exits 1 with `MachineBelowProfile { machine, field: ram_gib, required: 16, found: 8 }`.
- **AC-3** (error path) Given `ref-machines-no-recommended.toml`, then `NoMachineForProfile { profile: recommended }`.
- **AC-4** (error path) Given a `cloud_rented` machine without `instance_type`, then `MissingInstanceType`.
- **AC-5** Given a machine with a driver change dated between two release dates, then `is_comparable` is false; with no change, true.
- **AC-6** Given the shipped file, when the page generator runs with `--check`, then the committed page matches.

## Design

- **Registry command(s):** none; data file plus `xtask plat-hw validate-machines` and `xtask plat-hw machines-page [--check]`.
- **Data structures:** `RefMachines { schema_version, machines: Vec<Machine { name, profile, hosting: maintainer_owned | cloud_rented, host, instance_type: Option<String>, cpu_model, cores, cpu_level, ram_gib, gpu_model, gpu_class, vram_gib, storage_free_gib, display, os, os_version, gpu_driver, changes: Vec<Change { date, what }> }> }`.
- **Algorithm:** build an `EnvSnapshot` from each entry and call `fid-doctor::lookup::assess` against the profile file; read the `minimum` and `recommended` verdicts; comparability by date ranges.
- **Errors:** `MachineBelowProfile`, `NoMachineForProfile`, `MissingInstanceType`, `DuplicateName`, parse errors.
- **Determinism:** D0.
- **Crate:** types in `fid-platform` (module `ref_machines`); validation in `xtask` (it links `fid-doctor` for the verdict, avoiding a `fid-platform` → `fid-doctor` cycle).

## Test fixtures

Hand-written TOML under `fixtures/plat/hw/` with invented machine names; profile fixtures from `PLAT.HW.PROFILE`. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `ref_machines` parsing `platform/ref-machines.toml` (schema 1, `deny_unknown_fields`, 64 KiB cap): machines with name, profile, hosting (maintainer_owned | cloud_rented), host, instance_type (required for cloud_rented), CPU, cores, cpu_level, RAM, GPU model and class, VRAM, storage, display, OS and version, GPU driver, dated change log; provide `is_comparable(machine, date_a, date_b)`. In `xtask`, add `plat-hw validate-machines` (build an `EnvSnapshot` per machine and use `fid-doctor::lookup::assess` with `platform/hw-profiles.toml`; require a machine for `minimum` and `recommended`) and `plat-hw machines-page [--check]` generating `docs/benchmarks/reference-machines.md` with each host named. Write `docs/benchmarks/host-rules.md` (10-minute idle, no other workload, OS updates paused, run records the check). Ship the registry with the D-014 arrangement (maintainer-owned minimum machine, cloud-rented recommended instance) using placeholder specs marked provisional. Tests: AC-1..AC-6. Constraints: CONSTITUTION §8; `BTreeMap` only. Commit: `feat(fid-platform): reference machine registry`.

## Open questions

- The concrete machines and the rented instance type are maintainer choices under D-014; the shipped values are placeholders until named.

## Children

_Leaf: none._
