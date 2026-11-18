---
id: PLAT.HW.PROFILE
title: Hardware profile file and validator
level: 3
parent: PLAT.HW
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-platform]
built_pr: 95
satisfies: [REQ-PLAT.HW-001]
covers: []
depends_on: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.HW.PROFILE: Hardware profile file

## Purpose

Publishes the minimum and recommended hardware profiles as one versioned, machine-readable file (`platform/hw-profiles.toml`) with a strict schema and validator, so `fid doctor`, the docs and the benchmark tooling all read the same numbers.

## Scope

**In:**
- Schema, parser and validator for `platform/hw-profiles.toml`
- The `minimum` and `recommended` profiles (initial values as a proposal)
- The rule that the recommended profile is at least the minimum on every axis

**Out** (and which node owns it instead):
- Checking a machine against a profile (`PLAT.DOCTOR.LOOKUP`)
- Reference machines that embody the profiles (`PLAT.HW.REFMACH`)
- The frame-rate target that justifies the recommended profile (`PLAT.HW.SUITE`)
- GPU backend minimums (`PLAT.GPU.POLICY`)

## Requirements

- **REQ-PLAT.HW.PROFILE-001** The crate shall define a versioned profile schema whose profiles state CPU instruction-set level per architecture, physical core count, RAM in GiB, GPU class, VRAM in GiB, free storage in GiB and minimum display resolution.
- **REQ-PLAT.HW.PROFILE-002** The file shall contain exactly one profile named `minimum` and one named `recommended`, and may contain further named profiles.
- **REQ-PLAT.HW.PROFILE-003** If any numeric field of `recommended` is lower than that of `minimum`, or its GPU class ranks lower, then validation shall fail naming the field.
- **REQ-PLAT.HW.PROFILE-004** GPU class shall be one of `software`, `integrated`, `discrete_entry` or `discrete_mid`, ranked in that order.
- **REQ-PLAT.HW.PROFILE-005** If the file exceeds 64 KiB or has an unknown field, then parsing shall fail closed with a typed error.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/hw/profiles-valid.toml`, when `cargo xtask plat-hw validate` runs, then it exits 0 and prints the two profile names.
- **AC-2** (error path) Given `fixtures/plat/hw/profiles-inverted.toml` where recommended RAM (8) is below minimum RAM (16), when validated, then it exits 1 with `RecommendedBelowMinimum { field: ram_gib }`.
- **AC-3** (error path) Given `fixtures/plat/hw/profiles-no-minimum.toml`, when validated, then it exits 1 with `MissingProfile { name: minimum }`.
- **AC-4** (error path) Given `fixtures/plat/hw/profiles-extra-field.toml` with `ram_gb` instead of `ram_gib`, when validated, then it exits 1 with `UnknownField { field: ram_gb }`.
- **AC-5** Given the shipped `platform/hw-profiles.toml`, when validated in CI, then it exits 0.

## Design

- **Registry command(s):** none; internal library and `xtask plat-hw validate`.
- **Data structures:** `HwProfiles { schema_version: u32, profiles: BTreeMap<String, Profile> }`; `Profile { cpu_level: BTreeMap<Arch, CpuLevel>, cores: u32, ram_gib: u32, gpu_class: GpuClass, vram_gib: u32, storage_free_gib: u32, display: (u32, u32) }`. `CpuLevel` for x86_64 is `x86-64-v2 | x86-64-v3`; for aarch64 `armv8.0 | armv8.2`.
- **Initial proposal (all [unverified], to be confirmed by PLAT.HW.SUITE results):** minimum: x86-64-v2 or armv8.0, 4 cores, 16 GiB RAM, integrated GPU, 2 GiB shared VRAM, 20 GiB free, 1920×1080. Recommended: x86-64-v3 or armv8.2, 8 cores, 32 GiB RAM, discrete_mid GPU, 8 GiB VRAM, 100 GiB free, 2560×1440.
- **Algorithm:** parse with `deny_unknown_fields`; check required profiles; compare fields pairwise.
- **Errors:** `TooLarge`, `UnsupportedSchema`, `Parse`, `UnknownField`, `MissingProfile`, `RecommendedBelowMinimum`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `hw_profile`).

## Test fixtures

Hand-written TOML under `fixtures/plat/hw/`; licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `hw_profile` and `xtask plat-hw validate [--file <path>]`. Parse `platform/hw-profiles.toml` (cap 64 KiB, `schema_version` 1, unknown fields rejected): a table of named profiles with `cpu_level` per arch (x86_64: x86-64-v2 | x86-64-v3; aarch64: armv8.0 | armv8.2), `cores`, `ram_gib`, `gpu_class` (software < integrated < discrete_entry < discrete_mid), `vram_gib`, `storage_free_gib`, `display = [w, h]`. Require `minimum` and `recommended`; fail if recommended is below minimum on any field. Ship the initial file with the proposal values in the spec (marked as provisional in a comment).
>
> Acceptance tests: the five criteria above with fixtures in `fixtures/plat/hw/`. Constraints: CONSTITUTION §8; `BTreeMap` only. Commit: `feat(fid-platform): hardware profile file`.

## Open questions

- The proposal numbers are placeholders until the first release benchmark; changing them is a reviewed PR.

## Children

_Leaf: none._
