---
id: PLAT.DOCTOR.LOOKUP
title: Tier and hardware-profile verdicts
level: 3
parent: PLAT.DOCTOR
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.DOCTOR-001, REQ-PLAT.DOCTOR-002, REQ-PLAT.DOCTOR-005]
covers: []
depends_on: [PLAT.DOCTOR.PROBE, PLAT.MATRIX.FILE, PLAT.HW.PROFILE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.DOCTOR.LOOKUP: Tier and profile verdicts

## Purpose

The pure decision step of `fid doctor`: given an environment snapshot, the support matrix, the hardware profiles and the build's phase, it returns the platform's tier, a verdict per profile with every shortfall named, and one overall outcome that maps to an exit code. Being a pure function, it is tested exhaustively with snapshot fixtures.

## Scope

**In:**
- `assess(snapshot, matrix, profiles, phase) -> Assessment`
- Mapping probed GPU facts to the profile GPU classes
- The overall outcome and its exit-code mapping

**Out** (and which node owns it instead):
- Gathering facts (`PLAT.DOCTOR.PROBE`)
- Matrix and profile parsing (`PLAT.MATRIX.FILE`, `PLAT.HW.PROFILE`)
- Report formatting (`PLAT.DOCTOR.REPORT`)
- Process exit (`PLAT.DOCTOR.CMD`)

## Requirements

- **REQ-PLAT.DOCTOR.LOOKUP-001** When assessed, the function shall return the effective tier from the support matrix for the snapshot's OS, version and architecture at the given phase, or `unknown` if any of those items is unknown.
- **REQ-PLAT.DOCTOR.LOOKUP-002** For each profile in the profile file, the function shall return `meets`, `below` or `indeterminate`, and for `below` a list of shortfalls each naming the field, the required value and the found value.
- **REQ-PLAT.DOCTOR.LOOKUP-003** A profile verdict shall be `indeterminate` when no known item falls short but at least one required item is unknown, and the unknown items shall be listed.
- **REQ-PLAT.DOCTOR.LOOKUP-004** The function shall map the probed GPU to a profile GPU class as: fallback or CPU adapter → `software`; integrated → `integrated`; discrete with VRAM below 6 GiB → `discrete_entry`; discrete with at least 6 GiB → `discrete_mid` [unverified: VRAM boundary to be tuned].
- **REQ-PLAT.DOCTOR.LOOKUP-005** The overall outcome shall be, in order of precedence: `unsupported_platform` (tier not listed or only planned), `below_minimum`, `indeterminate` (minimum indeterminate), `meets_minimum` (recommended below or indeterminate), `meets_recommended`, with exit codes 3, 2, 4, 1 and 0 respectively.
- **REQ-PLAT.DOCTOR.LOOKUP-006** If no profile file is installed, then every profile verdict shall be `indeterminate` with reason `no_profiles` and the tier shall still be reported.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/doctor/env-win11-discrete.json`, `fixtures/plat/matrix/valid.toml`, `fixtures/plat/hw/profiles-valid.toml` and phase P1a, when `assess` runs, then tier is `tier1`, both profiles `meets`, outcome `meets_recommended`, exit code 0.
- **AC-2** Given `fixtures/plat/doctor/env-ubuntu-vm-software.json` (8 GiB RAM, software renderer), when assessed, then `minimum` is `below` with shortfalls `ram_gib` (16 required, 8 found) and `gpu_class` (integrated required, software found), and the outcome is `below_minimum`, exit code 2.
- **AC-3** Given `fixtures/plat/doctor/env-freebsd.json`, when assessed, then the outcome is `unsupported_platform`, exit code 3, and profile verdicts are still computed.
- **AC-4** Given `fixtures/plat/doctor/env-unknown-ram.json` where every other item meets the minimum, when assessed, then `minimum` is `indeterminate` listing `ram_gib`, outcome `indeterminate`, exit code 4.
- **AC-5** Given no profile file, when assessed with AC-1's snapshot, then tier is `tier1`, both verdicts `indeterminate(no_profiles)`.
- **AC-6** (determinism) Given AC-2's inputs, when assessed twice, then the serialised assessments are byte-identical.

## Design

- **Registry command(s):** none; library function used by `PLAT.DOCTOR.CMD`.
- **Data structures:** `Assessment { tier: Probed<EffectiveTier>, profiles: Vec<ProfileVerdict>, outcome: Outcome }`; `ProfileVerdict { name, verdict: meets | below | indeterminate, shortfalls: Vec<Shortfall { field, required, found }>, unknowns: Vec<field> }`; `Outcome` with `exit_code()`.
- **Algorithm:** matrix lookup through `fid-platform::matrix::lookup`; field-by-field comparison of the snapshot with each profile in a fixed field order (cpu_level, cores, ram_gib, gpu_class, vram_gib, storage_free_gib, display); profiles in file order (BTreeMap by name).
- **Errors:** none; missing inputs become `indeterminate` or `unknown`.
- **Determinism:** D0.
- **Performance budget:** < 1 ms.
- **Crate:** `fid-doctor` (module `lookup`).

## Test fixtures

Snapshot fixtures under `fixtures/plat/doctor/` (shared with PROBE), matrix fixtures from `PLAT.MATRIX.FILE` and profile fixtures from `PLAT.HW.PROFILE`.

## Build prompt

> In crate `fid-doctor`, add module `lookup` with a pure `assess(snapshot: &EnvSnapshot, matrix: &Matrix, profiles: Option<&HwProfiles>, phase: Phase) -> Assessment` using `fid-platform::{matrix, hw_profile}`.
>
> Tier: matrix lookup on OS family, version and arch at the phase; unknown if any is unknown. Per profile: compare cpu_level, cores, ram_gib, gpu_class, vram_gib, storage_free_gib (install volume) and display in that order; verdict `meets`, `below` (with shortfalls: field, required, found) or `indeterminate` (no shortfall but unknowns, listed). GPU class: fallback/CPU adapter → software; integrated → integrated; discrete < 6 GiB VRAM → discrete_entry; else discrete_mid. No profile file → all `indeterminate(no_profiles)`. Outcome precedence and exit codes: unsupported_platform 3, below_minimum 2, indeterminate 4, meets_minimum 1, meets_recommended 0.
>
> Acceptance tests: the six criteria above. Constraints: CONSTITUTION §8; D0 (no hash-ordered maps). Commit: `feat(fid-doctor): tier and profile verdicts`.

## Open questions

- Tier 3 platforms report their tier but are not `unsupported_platform`; whether Tier 3 should get a distinct exit code is left to user feedback.

## Children

_Leaf: none._
