---
id: PLAT.GPU.POLICY
title: GPU backend policy and known-bad driver list files
level: 3
parent: PLAT.GPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gpu-policy, fid-platform]
built_pr: 151
satisfies: [REQ-PLAT.GPU-001, REQ-PLAT.GPU-002, REQ-PLAT.GPU-006]
covers: []
depends_on: [PLAT.MATRIX.FILE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# PLAT.GPU.POLICY: Backend policy and known-bad driver list

## Purpose

Defines two published, machine-readable files and their validator: the backend policy (`platform/gpu-policy.toml`: for each OS and architecture, the backends tried in order and their minimums) and the known-bad driver list (`platform/gpu-known-bad.toml`: driver ranges with a workaround or forced fallback). VIZ, `PLAT.GPU.DETECT` and `fid doctor` read them through this crate.

## Scope

**In:**
- Schemas, parsers and validators for both files
- Cross-check against the support matrix: every Tier 1 and Tier 2 desktop combination has a policy row
- Encoding the mandatory backends of REQ-PLAT.GPU-002 as a validation rule
- The initial policy rows

**Out** (and which node owns it instead):
- Choosing a backend at run time (`PLAT.GPU.DETECT`)
- The fallback renderer itself (`PLAT.GPU.FALLBK`)
- Content research for known-bad entries beyond the format (maintained by contributors through PRs; each entry needs a reference)
- Browser graphics (`PLAT.WEB.GFX`)

## Requirements

- **REQ-PLAT.GPU.POLICY-001** The crate shall define a versioned policy schema with rows `{ os, arch, backends: ordered list, min: per-backend minimum API version and optional minimum driver version }`, where a backend is one of `vulkan`, `metal`, `dx12`, `gl`, `webgpu` or `software`.
- **REQ-PLAT.GPU.POLICY-002** If any effective Tier 1 or Tier 2 desktop combination in the support matrix has no policy row, then validation shall fail naming the combination.
- **REQ-PLAT.GPU.POLICY-003** If a Windows row omits `vulkan` or `dx12`, a Linux row omits `vulkan`, or a macOS row omits `metal`, then validation shall fail naming the row.
- **REQ-PLAT.GPU.POLICY-004** If a policy row does not end with `software`, then validation shall fail, so that every platform has a last-resort fallback.
- **REQ-PLAT.GPU.POLICY-005** The crate shall define a versioned known-bad schema with entries `{ id, os, backend, vendor_id, device_id range or any, driver version range, action, reason, reference }`, where action is `avoid_backend`, `force_software` or `workaround:<name>`.
- **REQ-PLAT.GPU.POLICY-006** If a known-bad entry lacks a `reference` URL or a `reason`, then validation shall fail naming the entry.
- **REQ-PLAT.GPU.POLICY-007** If either file exceeds 1 MiB or 10 000 entries, then parsing shall fail closed with `TooLarge`.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/gpu/policy-valid.toml` and `fixtures/plat/matrix/valid.toml`, when `cargo xtask plat-gpu validate` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/plat/gpu/policy-win-no-vulkan.toml`, when validated, then it exits 1 with `MissingMandatoryBackend { os: windows, backend: vulkan }`.
- **AC-3** (error path) Given `fixtures/plat/gpu/policy-missing-row.toml` lacking the macOS aarch64 row, when validated against `valid.toml` at phase P1a, then it exits 1 with `NoPolicyRow { os: macos, arch: aarch64 }`.
- **AC-4** (error path) Given `fixtures/plat/gpu/policy-no-software.toml`, when validated, then it exits 1 with `NoSoftwareFallback`.
- **AC-5** Given `fixtures/plat/gpu/known-bad-valid.toml` (3 entries), when validated, then it exits 0; given `known-bad-noref.toml`, it exits 1 with `MissingReference { id }`.
- **AC-6** Given the shipped `platform/gpu-policy.toml` and `platform/support-matrix.toml`, when validated in CI, then it exits 0.

## Design

- **Registry command(s):** none; internal library plus `xtask plat-gpu validate`. The files are read at run time by `PLAT.GPU.DETECT` and `PLAT.DOCTOR`.
- **Data structures:** `GpuPolicy { schema_version, rows: Vec<PolicyRow> }`; `PolicyRow { os, arch, backends: Vec<Backend>, min: BTreeMap<Backend, Minimum> }`; `Minimum { api: Version, driver: Option<Version> }`; `KnownBad { schema_version, entries: Vec<BadEntry> }`; `BadEntry { id, os, backend, vendor_id: u32, device_ids: IdRange, driver: VersionRange, action: BadAction, reason, reference }`.
- **Initial rows (proposal, all minimums [unverified]):** Windows x86-64: `dx12, vulkan, software` (DX12 feature level 11_0, Vulkan 1.1); Linux x86-64 and aarch64: `vulkan, software` (Vulkan 1.1); macOS aarch64: `metal, software` (Metal 2). The order on Windows (DX12 before Vulkan) is a proposal to be confirmed by `PLAT.GPU.CI` results.
- **Algorithm:** parse with size guard; per-row checks; join with `fid-platform::matrix` effective tiers at a given phase.
- **Errors:** `TooLarge`, `UnsupportedSchema`, `Parse`, `NoPolicyRow`, `MissingMandatoryBackend`, `NoSoftwareFallback`, `DuplicateBackend`, `MissingReference`, `MissingReason`, `BadRange`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-platform` (module `gpu_policy`).

## Test fixtures

Hand-written TOML under `fixtures/plat/gpu/`; reuse `fixtures/plat/matrix/valid.toml`. Known-bad fixtures use invented vendor and device IDs, never real driver data.

## Build prompt

> In crate `fid-platform`, add module `gpu_policy` and `xtask plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>] [--phase <P>]`.
>
> Parse and validate two TOML files. Policy rows: `os`, `arch`, ordered `backends` (vulkan, metal, dx12, gl, webgpu, software), and `min` per backend (`api` version, optional `driver` version). Rules: every effective Tier 1/Tier 2 desktop combination in the support matrix (via `fid-platform::matrix`) has a row; Windows rows include dx12 and vulkan, Linux rows vulkan, macOS rows metal; every row ends with `software`; no duplicate backends. Known-bad entries: `id`, `os`, `backend`, `vendor_id`, `device_ids` (range or `any`), `driver` version range, `action` (`avoid_backend`, `force_software`, `workaround:<name>`), `reason`, `reference` URL; reason and reference are mandatory. Both files: 1 MiB / 10 000-entry cap, `schema_version` 1.
>
> Ship initial `platform/gpu-policy.toml` with the proposal rows (Windows: dx12, vulkan, software; Linux: vulkan, software; macOS: metal, software) and an empty `platform/gpu-known-bad.toml`.
>
> Acceptance tests: the six criteria above. Constraints: CONSTITUTION §8; C2 (this data never influences values of record). Commit: `feat(fid-platform): GPU backend policy and known-bad list`.

## Open questions

- `webgpu` is the browser backend (`wgpu::Backends::BROWSER_WEBGPU`), which a desktop build does not expose: a desktop row that lists `webgpu` validates but is always rejected at probe time (`PLAT.GPU.DETECT` records it as a probe failure and moves on). The schema has no browser OS yet; a validation warning for `webgpu` on a desktop row, or a browser row, is left for when the browser build is specified.
- Minimum API versions are placeholders until `PLAT.GPU.CI` measures them on the minimum hardware profile.
- Where researched, referenced real known-bad entries come from (public bug trackers or vendor release notes, contributed through reviewed PRs). The shipped list is empty and the fixtures use invented IDs only.
- How vendor driver strings and the packed Vulkan `driverVersion` map to the dotted numeric `Version` used for matching; owned by `PLAT.GPU.DETECT`.
- `Minimum.driver` cannot be vendor-agnostic (driver numbering differs per vendor and OS); the shipped policy leaves it unset until a per-vendor form is decided.

## Decisions recorded at review (PR #151)

- **Precedence:** when several known-bad entries match, `force_software` > `avoid_backend` > `workaround:<name>`; ties go to the first entry in the file.
- **Safe defaults:** a missing or invalid policy file means software rendering only; an invalid known-bad file is treated as empty, with its diagnostics reported as a warning.

## Change log

- 2026-11-21 (maintainer decision on the layering of `PLAT.GPU.DETECT`): `gl` and `webgpu` join the backend set of REQ-001, so the policy can name every backend the frame graph (`VIZ.RENDER.FRAME.GRAPH`) selects; the mandatory-backend rules of REQ-003 and the `software`-last rule are unchanged, and neither new backend is mandatory in any row. The schema, validators and known-bad lookup move from `fid-platform` to crate `fid-gpu-policy` (layer model, below the tool layer, re-exported by `fid-platform`; refactor PR #371), so that `PLAT.GPU.DETECT` and VIZ can read them; `fid-platform` keeps the support-matrix cross-check. The code change adding the two backends to the `Backend` enum follows in a code PR after #371.

## Children

_Leaf: none._
