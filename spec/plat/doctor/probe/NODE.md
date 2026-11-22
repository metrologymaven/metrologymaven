---
id: PLAT.DOCTOR.PROBE
title: Environment probes
level: 3
parent: PLAT.DOCTOR
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-doctor]
satisfies: [REQ-PLAT.DOCTOR-001, REQ-PLAT.DOCTOR-006]
covers: []
depends_on: [PLAT.GPU.DETECT]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DOCTOR.PROBE: Environment probes

## Purpose

Gathers the facts `fid doctor` reasons about: OS and version, architecture, CPU, memory, free storage, display and GPU. Every probe returns either a value or `unknown` with a reason, runs under a deadline and never panics, so a broken driver or locked-down machine still yields a report. Probes are behind a trait so the rest of the doctor can be tested with recorded snapshots.

## Scope

**In:**
- An `Env` trait with one method per probe, a real implementation per Tier 1 OS and a fake implementation fed from JSON
- The `EnvSnapshot` type: one `Probed<T>` per item (`known(value)` or `unknown(reason)`)
- Per-probe deadlines and error capture
- Not collecting identifying data (user name, host name, serial numbers)

**Out** (and which node owns it instead):
- Tier and profile verdicts (`PLAT.DOCTOR.LOOKUP`)
- Report schema, rendering and redaction (`PLAT.DOCTOR.REPORT`)
- GPU adapter enumeration and selection logic (`PLAT.GPU.DETECT`; this node calls it)
- Driver-host and install-integrity checks (`PLAT.DOCTOR.HOSTCHK`)

## Requirements

- **REQ-PLAT.DOCTOR.PROBE-001** The probe set shall gather OS family and version, architecture, CPU model, physical core count, CPU instruction-set level, total and available RAM, free storage on the install volume and on the data volume, primary display resolution, and the `GpuSelection` (backend, adapter, driver version, fallback flag).
- **REQ-PLAT.DOCTOR.PROBE-002** If a probe returns an error, exceeds its 2-second deadline or is unsupported on the OS, then its item shall be `unknown` with a reason code (`error`, `timeout`, `unsupported`, `no_display`, `not_built`) and the remaining probes shall still run.
- **REQ-PLAT.DOCTOR.PROBE-003** The probes shall not read or return the user name, host name, machine serial numbers or MAC addresses.
- **REQ-PLAT.DOCTOR.PROBE-004** The probes shall make no network request.
- **REQ-PLAT.DOCTOR.PROBE-005** The fake `Env` shall load an `EnvSnapshot` from JSON so that every downstream test can run without real hardware.
- **REQ-PLAT.DOCTOR.PROBE-006** If the build does not include GPU support (P0 builds, or headless builds), then the GPU items shall be `unknown(not_built)` rather than an error.

## Acceptance criteria

- **AC-1** Given a Tier 1 CI runner, when `probe_all(RealEnv, deadline)` runs, then OS family, version, architecture, core count and total RAM are `known`, and the snapshot serialises to JSON validating against `fid-doctor`'s snapshot schema.
- **AC-2** Given a fake `Env` whose RAM probe returns an error and whose display probe never returns, when `probe_all` runs with a 2 s per-probe deadline, then RAM is `unknown(error)`, display is `unknown(timeout)`, every other item is `known`, total time is < 3 s, and no panic occurs.
- **AC-3** Given `fixtures/plat/doctor/env-win11-discrete.json`, when loaded through the fake `Env`, then the resulting snapshot round-trips to byte-identical JSON.
- **AC-4** Given a headless Linux CI runner without a display, when `probe_all` runs, then display is `unknown(no_display)`.
- **AC-5** Given a build without the `gpu` feature, when `probe_all` runs, then the GPU items are `unknown(not_built)`.
- **AC-6** Given a real-environment run on each Tier 1 runner, when the snapshot JSON is searched for the runner's host name and user name (read by the test from the OS), then neither appears.

## Design

- **Registry command(s):** none; library used by `PLAT.DOCTOR.CMD` and `PLAT.HW.CMD`.
- **Data structures:** `enum Probed<T> { Known(T), Unknown { reason: UnknownReason, detail: String } }`; `EnvSnapshot { os: Probed<OsInfo>, arch: Probed<Arch>, cpu: Probed<CpuInfo>, ram: Probed<RamInfo>, storage: Probed<Vec<VolumeInfo>>, display: Probed<(u32, u32)>, gpu: Probed<GpuSelection> }`. `VolumeInfo { role: install | data, free_gib, total_gib }` (no paths). `detail` strings are limited to 200 chars.
- **Algorithm:** each probe runs on a worker thread with a deadline; the caller collects results in a fixed order. Windows version via the OS version API (build number), Linux via `/etc/os-release`, macOS via the system version property; CPU level by feature detection (x86-64-v2/v3, armv8.x); RAM and storage via a system-information crate; GPU via `fid-gpu-select::probe` + `select` with the installed policy files.
- **Errors:** none escape; all become `Unknown`.
- **Determinism:** N (observes the machine); the fake `Env` path is D0.
- **Performance budget:** all probes < 5 s on the minimum profile, GPU probe included [unverified].
- **Crate:** `fid-doctor` (module `probe`); optional feature `gpu` pulls in `fid-gpu-select`.

## Test fixtures

`fixtures/plat/doctor/env-*.json` snapshots written by hand (Windows 11 discrete GPU, Ubuntu VM with software renderer, macOS ARM laptop, a snapshot with several unknowns); licensed with the code.

## Build prompt

> Create crate `fid-doctor` with module `probe`. Define `Probed<T>` (`Known(T)` or `Unknown { reason, detail }`, reasons: error, timeout, unsupported, no_display, not_built) and `EnvSnapshot` with OS (family, version), arch, CPU (model, physical cores, instruction-set level), RAM (total, available), storage (install and data volumes: free and total GiB, no paths), primary display resolution and GPU (`fid-gpu-select::GpuSelection`, behind cargo feature `gpu`; without it `Unknown(not_built)`).
>
> Implement an `Env` trait with one method per probe; `RealEnv` for Windows, Linux and macOS; `FakeEnv` loaded from JSON. `probe_all(env, per_probe_deadline)` runs each probe on a worker thread with a deadline (default 2 s), converts every error, timeout or unsupported case into `Unknown`, never panics, and returns the snapshot in a fixed field order. Never read user name, host name, serial numbers or MAC addresses; make no network request.
>
> Acceptance tests: the six criteria above, with fixtures in `fixtures/plat/doctor/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe` in this crate; dependencies may use it), C7 (offline). Commit: `feat(fid-doctor): environment probes`.

## Open questions

- Detecting the physical (not logical) core count reliably on every OS may need per-OS code [unverified].

## Children

_Leaf: none._
