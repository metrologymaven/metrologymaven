---
id: PLAT.GPU.DETECT
title: GPU adapter probing and backend selection
level: 3
parent: PLAT.GPU
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gpu-select]
built_pr: 382
satisfies: [REQ-PLAT.GPU-003, REQ-PLAT.GPU-006, REQ-PLAT.GPU-008]
covers: []
depends_on: [PLAT.GPU.POLICY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
---

# PLAT.GPU.DETECT: Adapter probing and backend selection

## Purpose

Decides which GPU backend and adapter the app uses, or that it must fall back. A thin probe gathers adapter facts from the graphics API; a pure selection function applies the backend policy, the minimums and the known-bad list and returns a `GpuSelection` that VIZ uses to create its device, the notice uses to explain itself and `fid doctor` reports. Keeping selection pure makes every rule testable with recorded adapter lists.

## Scope

**In:**
- The `AdapterInfo` and `GpuSelection` types (the "backend-detection result type" offered by PLAT.GPU)
- A probe that lists adapters per backend through wgpu without creating a rendering surface
- The pure selection function: policy order, minimums, known-bad actions, optional user override
- Recording why each adapter was rejected

**Out** (and which node owns it instead):
- The fallback renderer (`PLAT.GPU.FALLBK`)
- The notice UI (`PLAT.GPU.NOTICE`)
- Creating the device and swapchain (`VIZ`)
- Reading the user override setting from the settings store (`CORE.STO`; passed in as a parameter)

## Requirements

- **REQ-PLAT.GPU.DETECT-001** When given a list of adapters, the policy row for the current OS and architecture, and the known-bad list, the selection function shall return the first adapter, in policy backend order and then hardware-before-integrated-before-other order, that meets the backend minimum and is not excluded by a known-bad entry.
- **REQ-PLAT.GPU.DETECT-002** If no hardware adapter qualifies, then the selection shall return `software` with `fallback = true` and a reason listing each rejected adapter and its rejection cause.
- **REQ-PLAT.GPU.DETECT-003** When an adapter matches a known-bad entry with action `avoid_backend`, the selection shall skip that adapter on that backend; with `force_software`, it shall select `software`; with `workaround:<name>`, it shall select the adapter and list the workaround name; and each applied entry ID shall appear in the result.
- **REQ-PLAT.GPU.DETECT-004** When the caller passes a backend override that the policy row allows, the selection shall try that backend first, except where a known-bad `force_software` entry matches.
- **REQ-PLAT.GPU.DETECT-005** If the probe fails or times out after 5 seconds for a backend, then the selection shall treat that backend as unavailable and record the reason, and shall not panic [unverified: timeout to be tuned].
- **REQ-PLAT.GPU.DETECT-006** The crate shall not be a dependency of any kernel crate, and the layering check shall fail if it becomes one (CONSTITUTION C2).
- **REQ-PLAT.GPU.DETECT-007** `GpuSelection` shall serialise to JSON with fields backend, adapter name, vendor ID, device ID, driver version, fallback flag, reason, applied known-bad IDs and workarounds.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/gpu/adapters-win-discrete.json` (one discrete DX12 and Vulkan adapter, one integrated) and the Windows row of `fixtures/plat/gpu/policy-valid.toml`, when `select` runs, then the result is `dx12` on the discrete adapter with `fallback = false`.
- **AC-2** Given `fixtures/plat/gpu/adapters-vm-none.json` (only a basic display adapter below every minimum), when `select` runs, then the result is `software`, `fallback = true`, and the reason names the adapter and `below_minimum`.
- **AC-3** Given `adapters-win-discrete.json` and `fixtures/plat/gpu/known-bad-valid.toml` with an `avoid_backend` entry for that discrete adapter on dx12, when `select` runs, then the result is `vulkan` on the same adapter and lists the entry ID.
- **AC-4** Given a known-bad `force_software` match and a user override of `vulkan`, when `select` runs, then the result is `software` and lists the entry ID.
- **AC-5** Given a probe stub that never returns for Vulkan, when probing runs with a 5 s timeout, then selection completes, Vulkan is listed as `probe_timeout`, and no panic occurs.
- **AC-6** Given the workspace, when the layering check runs, then no crate under the kernel graph depends on `fid-gpu-select`.
- **AC-7** (determinism) Given AC-1's inputs, when `select` runs twice, then the serialised `GpuSelection` is byte-identical.

## Design

- **Registry command(s):** none; library consumed by VIZ and `PLAT.DOCTOR.PROBE`.
- **Data structures:** `AdapterInfo { backend, name, vendor_id, device_id, device_type: discrete | integrated | virtual | cpu | other, driver_version: Option<Version>, api_version: Version }`; `GpuSelection` as in REQ-007; `Rejection { adapter, backend, cause: below_minimum | known_bad(id) | probe_failed(msg) | probe_timeout }`.
- **Algorithm:** probe each backend in the policy row in a worker thread with a deadline; collect `AdapterInfo`; sort candidates by (policy backend index, device-type rank, original index) with a stable sort; apply override; evaluate minimums and known-bad entries in order; first survivor wins; otherwise software.
- **Errors:** probe errors become `Rejection`s, never returned errors; only an invalid policy file is an error (`PolicyError`).
- **Determinism:** D0 for `select` given its inputs; the probe itself is observed (`N`) and its output is recorded in the result so selection can be replayed.
- **Performance budget:** selection < 1 ms; probing < 2 s on the minimum profile [unverified until measured].
- **Crate:** `fid-gpu-select`, layer viewer (depends on `fid-gpu-policy` and `wgpu`, never on the tool-layer `fid-platform`; outside the kernel graph). VIZ consumes the result as plain data: `VIZ.RENDER.FRAME.GRAPH` takes an `AdapterChoice` (backend, vendor ID, device ID, name), so neither node depends on the other.

## Test fixtures

JSON adapter lists under `fixtures/plat/gpu/` (`adapters-win-discrete.json`, `adapters-vm-none.json`, more as needed), written by hand from the `AdapterInfo` schema with invented IDs; policy and known-bad fixtures shared with `PLAT.GPU.POLICY`.

## Build prompt

> Create crate `fid-gpu-select` (layer viewer; depends on `fid-gpu-policy` and `wgpu`). Provide `probe(policy_row, timeout) -> Vec<ProbeOutcome>` that enumerates adapters through wgpu for each backend in the row, each on a worker thread with a deadline, without creating a surface; and a pure `select(adapters, policy_row, known_bad, override) -> GpuSelection`.
>
> Selection: candidates ordered by policy backend order, then device type (discrete, integrated, virtual, other, cpu), then original index, stable. Override (if allowed by the row) moves its backend first. Reject adapters below the row's minimum API or driver version; apply known-bad entries (`avoid_backend` skips, `force_software` selects software, `workaround:<name>` keeps the adapter and lists the workaround). No survivor gives `software` with `fallback = true` and a reason listing every rejection. Record applied known-bad IDs. Serialise `GpuSelection` to JSON (backend, adapter name, vendor ID, device ID, driver version, fallback, reason, known_bad IDs, workarounds).
>
> Acceptance tests: the seven criteria above with fixtures in `fixtures/plat/gpu/`. Constraints: CONSTITUTION §8 (no panic, no `unwrap`, no `unsafe`); C2: the crate must never be in the kernel dependency graph; add it to the layering check's display-only list. Commit: `feat(fid-gpu-select): adapter probing and backend selection`.

## Open questions

- Build: wgpu (27) reports neither an API version nor a numeric driver version. **The API minimum is therefore only partly enforced on real hardware.** `WgpuProber` records a guaranteed floor, `AdapterInfo::api_floor`, where wgpu exposes a usable proxy: DX12 11.0 (wgpu-hal creates the device at feature level 11_0 and hides an adapter that cannot), or 11.1 when wgpu reports `Features::VERTEX_WRITABLE_STORAGE` (set on DX12 only from feature level 11_1); Vulkan 1.0. Feature level 12.x and Vulkan 1.1 and later are not visible through wgpu's downlevel capabilities, limits or features. A minimum at or below the floor is verified; above it (the Windows row's `vulkan` 1.1, for example) the backend's API minimum is `unverified`: listed in `GpuSelection::unverified` and never a rejection by itself. An exact `api_version` (recorded fixtures) below the minimum rejects as `below_minimum`. Known-bad entries apply whatever the minimum's state. An unknown driver version is handled the same way, and a known-bad entry with a driver range applies to an adapter whose driver version is unknown (the safe side), also listed there. Enforcing the API minimum exactly needs native API queries beyond wgpu (follow-up).
- Build: the driver version is the **first dotted numeric token** of the adapter's `driver_info` (of `driver` only when `driver_info` is empty), and only that token: if it touches a letter (AMDVLK's `2024.Q2.1 (LLPC)`) or is not a 1-to-4-component version (`Mesa 24.0.5.1.2 (LLVM 17.0.6)`), the version is unknown, never a later token. GL-style strings put the API version first (`4.6.0 NVIDIA 560.94` reads as 4.6.0), so the `gl` backend needs its own rule when it is mapped. Mapping vendor driver strings and the packed Vulkan `driverVersion` exactly (a POLICY open question owned here) needs per-vendor rules and native API queries beyond wgpu.
- Build: a CPU adapter on a hardware backend (lavapipe on Vulkan, WARP on DX12) is never selected as that backend; it is rejected as `not_hardware`, because REQ-002 makes `software` the answer when no hardware adapter qualifies. The Build prompt's `cpu` rank (last) therefore never decides. The frame graph's `software` backend then picks the CPU adapter itself.
- Build: an allowed `software` override (REQ-004; every valid row ends with `software`) selects `software` at once, with reason `user_override` and `fallback = true` (no hardware adapter is in use), without evaluating any adapter or known-bad entry; `Probe::run_with_override` then probes nothing, so a user whose GPU driver crashes or hangs can escape it. Probe rejections passed in are still carried. An override the row does not list is ignored and noted in `unverified`, rather than refused. Applied known-bad IDs list every entry that changed the result (`avoid_backend` skips, then `force_software` or the workarounds), in the order they applied; when several entries match one adapter, the strongest action wins (`force_software` > `avoid_backend` > `workaround`, ties in file order, as `PLAT.GPU.POLICY`), and every matching workaround is listed.
- Build: AC-3 and AC-4 use new fixtures `fixtures/plat/gpu/known-bad-detect.toml` and `known-bad-detect-force.toml`; `known-bad-valid.toml` belongs to `PLAT.GPU.POLICY`'s AC-5 (three entries) and has no DX12 entry, so AC-3 as written names the wrong fixture.
- **AC-3 depends on its fixture, and the backend-first order is open.** In `adapters-win-discrete.json` the integrated adapter is on Vulkan only, and only that makes AC-3's expected answer (Vulkan on the same discrete adapter) follow from REQ-001. With the integrated adapter on DX12 too, REQ-001's backend-first order (policy backend, then device type) picks DX12 on the integrated adapter, not Vulkan on the discrete one. Whether an `avoid_backend` on the preferred adapter should keep that adapter on the next backend (device first) or keep the backend on the next adapter (backend first, as built) is a spec decision for this node; the build follows REQ-001 as written.
- Build: the probe runs every hardware backend of the row in parallel, each on its own thread, against one deadline from the call (5 s, `DEFAULT_PROBE_TIMEOUT`); a backend that does not answer is `probe_timeout` and its thread is left running detached (it cannot be cancelled). A probe that panics inside the graphics layer is `probe_failed`. A hung driver is contained: `Probe` keeps one in-flight flag per backend (process-wide for the real driver, `Probe::wgpu()`), and a backend still inside the driver from an earlier probe is `probe_timeout` at once, with no new thread. `GpuSelection` lists the backends that timed out (`probe_timeout`) and failed (`probe_failed`), and so does `ProbeReport` (`timed_out()`, `failed()`).
- **Frame-graph follow-up (lane VIZ.RENDER.FRAME.GRAPH, separate PR):** the frame graph (`fid-render`, #368's `WgpuSource::new`, `mod.rs:133`) must create its wgpu instance with only the selected backend, never all backends; otherwise a backend whose probe hung or failed is touched again when the device is created, and the containment above is lost. Not changed here.
- Build: there is no `metal` feature in this crate's wgpu dependency (the same features as `fid-render`), so on macOS `metal` is reported `probe_failed` ("not compiled into this build") and the selection falls back to `software` until the `metal` feature is vetted against `deny.toml`. `GpuSelection` serialises as canonical JSON (`gpu-selection@1`, IDs as `0x` hex) and `adapter_choice()` gives the plain-data choice `VIZ.RENDER.FRAME.GRAPH` takes. `gl` and `webgpu` (#380) are not probed until they are mapped in `WgpuProber`.
- Build: AC-6 is checked twice: the layer gate (`fid-gpu-select` is `viewer` in `layers.toml`, so no `num` or `kernel` crate may reach it) and a test that walks the resolved `cargo metadata` graph from every kernel-graph crate, asserting that it found a root for every `num` and `kernel` crate in `layers.toml`. There is no separate "display-only list" in the layering check; the layer map is that list.
- Build: AC-7 compares the serialised selection byte for byte with the hand-written golden file `fixtures/plat/gpu/selection-win-discrete.golden.json` (written from the rules above; its key order checked with Python's `json.dumps(sort_keys=True)`, not by running the crate).

- Whether the probe can run without a window on every platform (notably macOS) needs a spike [unverified].

## Change log

- 2026-11-21 (maintainer decision on the layering of this node): `depends_on` drops `VIZ`, which made a cycle with `VIZ.RENDER.FRAME.GRAPH` (that leaf takes this node's result as an injected `AdapterChoice`, plain data, with no code edge either way); the real dependency is `PLAT.GPU.POLICY` only. The crate depends on `fid-gpu-policy` (the policy schema and known-bad lookup moved below the tool layer, refactor PR #371) instead of `fid-platform`, a tool-layer crate nothing may depend on, and sits in the viewer layer. `gl` and `webgpu` are policy backends (`PLAT.GPU.POLICY` change log).

## Children

_Leaf: none._
