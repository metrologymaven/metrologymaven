---
id: KRN.NUM.LINT.FLAGS
title: Kernel build-configuration and emitted-code FMA check
level: 4
parent: KRN.NUM.LINT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM.LINT-003, REQ-KRN.NUM.LINT-007]
covers: []
depends_on: [KRN.NUM.LINT.FIND]
uses_artifacts: [KRN.MAN.WASM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM.LINT.FLAGS: Kernel build-configuration and emitted-code FMA check

## Purpose

Source can be clean while the build still changes results: `target-cpu=native` or an `+fma` target feature lets the compiler use fused instructions, several codegen units can change inlining, and an unpinned toolchain changes code generation between runs. This leaf checks every place the kernel build takes flags from, and, because flags alone may not exclude every fused instruction, also scans the emitted wasm reference module for fused or relaxed multiply-add opcodes.

## Scope

**In:**
- Inputs: `rust-toolchain.toml`, `.cargo/config.toml` (all `rustflags`, `target.<triple>.rustflags`, `build.rustflags`), workspace `Cargo.toml` profiles used for kernel builds, and the CI workflow environment's `RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS` as declared in the lint config
- Rules: `KNP1-FLAG-NATIVE` (`target-cpu=native`), `KNP1-FLAG-FMA` (`+fma`, `+relaxed-simd`, or any `target-feature` enabling fused multiply-add), `KNP1-FLAG-CGU` (codegen-units other than 1 in a kernel profile), `KNP1-FLAG-TOOLCHAIN` (channel not an exact version, or no components pin), `KNP1-FLAG-LTO` (profile differences between targets that the lint config does not list as allowed)
- Emitted-code check `KNP1-FLAG-WASMFMA`: parse the wasm reference module and fail on relaxed-SIMD madd/nmadd opcodes or any SIMD opcode not allowed by the config
- Seeded fixtures per flag rule

**Out** (and which node owns it instead):
- Building the wasm module and its import allowlist (`KRN.MAN.WASM`); reproducible-build settings themselves (`KRN.MAN.REPRO`)
- Source-level FMA detection (`KRN.NUM.LINT.SYN`, `KRN.NUM.LINT.TYPED`)

## Requirements

- **REQ-KRN.NUM.LINT.FLAGS-001** If any configured flag source enables `target-cpu=native` for a kernel build, then the check shall report `KNP1-FLAG-NATIVE` with the file and line.
- **REQ-KRN.NUM.LINT.FLAGS-002** If any configured flag source enables a target feature that permits fused multiply-add (`+fma`, `+relaxed-simd`, or a feature listed as FMA-enabling in the lint config), then the check shall report `KNP1-FLAG-FMA`.
- **REQ-KRN.NUM.LINT.FLAGS-003** If a profile used for kernel builds sets `codegen-units` to anything other than 1 or leaves it at a default other than 1, then the check shall report `KNP1-FLAG-CGU`.
- **REQ-KRN.NUM.LINT.FLAGS-004** If `rust-toolchain.toml` is missing or its channel is not an exact version (for example `1.xx.y`), then the check shall report `KNP1-FLAG-TOOLCHAIN`.
- **REQ-KRN.NUM.LINT.FLAGS-005** When the wasm reference module is present, the check shall parse every function body and shall report `KNP1-FLAG-WASMFMA` for each relaxed-SIMD multiply-add opcode, and fail closed with `KNP1-FLAG-WASMPARSE` if the module cannot be parsed.
- **REQ-KRN.NUM.LINT.FLAGS-006** Each rule shall have a seeded fixture that the check detects.

## Acceptance criteria

- **AC-1** Given `fixtures/knp1/flags/native/` (`.cargo/config.toml` with `rustflags = ["-C", "target-cpu=native"]`), when the check runs, then `KNP1-FLAG-NATIVE` names the file and line.
- **AC-2** Given `fixtures/knp1/flags/fma/` (`target.x86_64-unknown-linux-gnu.rustflags = ["-C", "target-feature=+avx2,+fma"]`), then `KNP1-FLAG-FMA`.
- **AC-3** Given a kernel profile without `codegen-units = 1`, then `KNP1-FLAG-CGU`; given `channel = "stable"`, then `KNP1-FLAG-TOOLCHAIN`.
- **AC-4** Given `fixtures/knp1/flags/wasm/relaxed.wasm` (a hand-assembled module with one `f64x2.relaxed_madd`), then one `KNP1-FLAG-WASMFMA` finding; given `clean.wasm`, none; given a truncated module, `KNP1-FLAG-WASMPARSE`.
- **AC-5** Given `fixtures/knp1/flags/ok/` (pinned toolchain, `codegen-units = 1`, no target flags), then no findings.

## Design

- **Registry command(s):** none; module `flags` of tooling crate `fid-knp1-lint`.
- **Data structures:** lint-config section `[flags] profiles = [...]`, `ci_env_files = [...]`, `fma_features = [...]`, `wasm_module = "<path>"`, `allowed_simd = [...]`.
- **Algorithm:** parse TOML files with line tracking; split rustflags into `-C key=value` pairs; evaluate per profile with Cargo's inheritance rules for `codegen-units`; parse the wasm module with a streaming wasm parser and walk operators.
- **Errors:** `KNP1-FLAG-*` findings; `KNP1-FLAG-WASMPARSE` (fatal finding).
- **Determinism:** D0.
- **Performance budget:** < 2 s including a 10 MB wasm module [unverified until measured].
- **Crate:** `fid-knp1-lint` (tooling layer).

## Test fixtures

`fixtures/knp1/flags/`: config directories and small wasm modules (hand-written in text format and assembled by the test build); project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `flags`. Read `rust-toolchain.toml`, `.cargo/config.toml` (all rustflags keys), the kernel profiles in `Cargo.toml`, and the CI env files named in `knp1-lint.toml`. Report `KNP1-FLAG-NATIVE` (`target-cpu=native`), `KNP1-FLAG-FMA` (`+fma`, `+relaxed-simd`, or configured FMA-enabling features), `KNP1-FLAG-CGU` (kernel profile `codegen-units` != 1, including defaults), `KNP1-FLAG-TOOLCHAIN` (missing file or channel not an exact version), with file and line. Parse the wasm reference module named in the config (built by KRN.MAN.WASM) with a streaming wasm parser and report `KNP1-FLAG-WASMFMA` for relaxed-SIMD madd/nmadd opcodes; fail closed with `KNP1-FLAG-WASMPARSE`. Emit `findings::Finding`. Tests: `fixtures/knp1/flags/` for AC-1..AC-5 of spec node KRN.NUM.LINT.FLAGS. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`, bounded parsers.

## Open questions

- Native x86-64 and ARM64 object code is not scanned for fused instructions (`vfmadd*`, `fmadd`). Rust does not contract floating-point operations by default [unverified per target]; if a native artifact ever becomes a gate (KRN.MAN `native_gate`), add a disassembly check as a new rule.

## Children

_Leaf: none._
