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
built_in: [fid-knp1-lint]
built_pr: 182
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

### Review decisions (PR #182, 2026-11-19)

- **CI files and scripts are gated by a fail-closed substring scan.** Shell and YAML grammar have too many ways to build a flag string for a parser to converge. So every line of a `ci_env_files` entry is lowercased, `_` is mapped to `-`, and whitespace runs are collapsed. The line is then a finding if it contains any of:
  - `target-cpu`, `target-feature`, `codegen-units`, `llvm-args`, `fp-contract` or `-ctarget`;
  - `rustflags`, `rustdocflags` or `cargo-encoded-rustflags`;
  - `rustup run`, `rustup override`, `rustup default` or `rustup-toolchain`;
  - a `+` toolchain selector (`cargo +`, `rustc +`, `cargo.exe +`, `rustc.exe +`);
  - a `--config` whose value names a `profile`, `build` or `target` key, a file, a variable, or nothing.

  The structured parser only refines the rule and column. A line it does not explain is reported as `KNP1-FLAG-KEYWORD`.
- **Opt-out marker.** A line opts out only with a same-line marker `knp1-flags: allow <RULE> <reason>`. The marker suppresses that one rule on that one line and is reported as an informational `KNP1-FLAG-OPTOUT` note, which does not fail the lint.
- **`.cargo/config(.toml)` gates.** The free-form `[alias]`, `[env]` and `[target.*]` values (everything but the rustflags keys, which are parsed) go through the same substring scan. Having both `.cargo/config` and `.cargo/config.toml` is `KNP1-FLAG-AMBIGUOUS`.
- **Compiler wrappers (re-review of PR #235).** A `.cargo/config(.toml)` that sets `build.rustc-wrapper`, `build.rustc-workspace-wrapper` or `build.rustc` (either spelling) wraps or replaces the pinned compiler, which would also drop the type-aware check: `KNP1-FLAG-WRAPPER` at the key, never waivable and not covered by an opt-out marker.
- **CPU and wasm rules.** Any `target-cpu` that is not `native` and not in `allowed_target_cpus` is reported: `KNP1-FLAG-FMA` when the CPU is on the built-in FMA list below, otherwise `KNP1-FLAG-CPU`. Listing a built-in FMA CPU in `allowed_target_cpus` is a config error unless `[flags.fma_cpu_overrides] <cpu> = "<reason>"` gives a reason. `KNP1-FLAG-WASM*` findings cannot be waived.

### Built-in FMA-implying lists and their sources

All entries are [unverified] against a specific LLVM release; re-check them when the pinned toolchain moves.

- **Target features** (`+feature`):
  - `fma` (x86 FMA3) and `fma4` (AMD FMA4): LLVM `X86.td` features of those names.
  - Every `avx512*` and `avx10*` feature: in LLVM `X86.td` they imply `avx512f`, which implies `fma`.
  - `relaxed-simd`: WebAssembly relaxed-SIMD proposal, which has the `relaxed_madd`/`relaxed_nmadd` instructions.
  - `avx2` is not listed, because LLVM's `avx2` does not imply `fma`.
  - AArch64 has no FMA feature to gate. `FMLA`/`FMADD` are baseline FP/AdvSIMD instructions (Arm ARM), so there FMA is excluded only by rustc not contracting floating-point operations, together with the source checks (`KRN.NUM.LINT.SYN`, `KRN.NUM.LINT.TYPED`).
- **CPUs** (`-C target-cpu=`):
  - x86 microarchitecture levels `x86-64-v3` and `x86-64-v4` (System V x86-64 psABI micro-architecture levels: v3 includes FMA).
  - Intel `core-avx2`, `haswell` and every later Core/Xeon/Atom-E-core name in LLVM `X86.td`, plus Xeon Phi `knl` and `knm`. The full list is `BUILTIN_FMA_CPUS` in `crates/fid-knp1-lint/src/flags/mod.rs`. Their LLVM processor definitions include `FeatureFMA`.
  - AMD `bdver1` to `bdver4` (FMA4, plus FMA3 from `bdver2`) and every `znver*` (FMA3), per LLVM `X86.td`.
  - AArch64 `apple-*`, `neoverse-*`, `cortex-a*`, `cortex-x*`, `exynos-*`, `a64fx`, `carmel`, `thunderx2t99`, `thunderx3t110`, `ampere1` and `ampere1a`. All are LLVM `AArch64Processors.td` cores, and every AArch64 core has baseline FMA.

## Test fixtures

`fixtures/knp1/flags/`: config directories and small wasm modules (hand-written in text format and assembled by the test build); project licence.

## Build prompt

> In tooling crate `fid-knp1-lint`, add module `flags`. Read `rust-toolchain.toml`, `.cargo/config.toml` (all rustflags keys), the kernel profiles in `Cargo.toml`, and the CI env files named in `knp1-lint.toml`. Report `KNP1-FLAG-NATIVE` (`target-cpu=native`), `KNP1-FLAG-FMA` (`+fma`, `+relaxed-simd`, or configured FMA-enabling features), `KNP1-FLAG-CGU` (kernel profile `codegen-units` != 1, including defaults), `KNP1-FLAG-TOOLCHAIN` (missing file or channel not an exact version), with file and line. Parse the wasm reference module named in the config (built by KRN.MAN.WASM) with a streaming wasm parser and report `KNP1-FLAG-WASMFMA` for relaxed-SIMD madd/nmadd opcodes; fail closed with `KNP1-FLAG-WASMPARSE`. Emit `findings::Finding`. Tests: `fixtures/knp1/flags/` for AC-1..AC-5 of spec node KRN.NUM.LINT.FLAGS. Constraints (CONSTITUTION §8): no `unsafe`, deny `unwrap`/`expect`/`panic`, bounded parsers.

## Open questions

- Native x86-64 and ARM64 object code is not scanned for fused instructions (`vfmadd*`, `fmadd`). Rust does not contract floating-point operations by default [unverified per target]; if a native artifact ever becomes a gate (KRN.MAN `native_gate`), add a disassembly check as a new rule.
- Accepted limits of the build-configuration check (PR #182):
  - **Per-crate configs and toolchain files.** `.cargo/config(.toml)` and `rust-toolchain(.toml)` files in member-crate directories, parent directories or `$CARGO_HOME` are not read. Only the workspace directory is checked.
  - **Unlisted scripts and actions.** Scripts, Makefiles and composite or reusable GitHub actions that are not listed in `ci_env_files` are not scanned, and neither are the inputs of called actions (for example a `toolchain:` input).
  - **Variable indirection.** A flag assembled from variables whose names and values avoid every keyword (for example `F=target; G=-cpu`) is not detected.
  - **Custom target JSON.** A `--target <file>.json` specification can set `cpu`, `features` or LLVM arguments, and it is not read.

## Children

_Leaf: none._
