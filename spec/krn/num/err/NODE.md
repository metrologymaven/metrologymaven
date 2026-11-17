---
id: KRN.NUM.ERR
title: Numeric error type and FP-environment checks
level: 3
parent: KRN.NUM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-007]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.NUM.ERR: Numeric error type and FP-environment checks

## Purpose

The single typed error that every `fid-num` operation returns, the finiteness guards that turn NaN and infinity into that error, and the floating-point environment probe run at kernel entry. Every other `fid-num` module depends on it, so it is built first.

## Scope

**In:** the `NumError` enum; `check_finite` guards for scalars and slices; a pure-arithmetic probe that detects flush-to-zero, denormals-are-zero and non-default rounding.
**Out:** errors of solvers (`KRN.SOLV`), fits (`KRN.LSQ`) and the kernel call (`KRN.MAN.KCI`), which wrap `NumError`; the lint (`KRN.NUM.LINT`).

## Requirements

- **REQ-KRN.NUM.ERR-001** The module shall define one `NumError` enum with at least the variants `NotANumber`, `Infinite`, `Domain`, `Singular`, `EmptyInput`, `LengthMismatch` and `FpEnvironment`, each carrying the operation name and the offending index where one exists.
- **REQ-KRN.NUM.ERR-002** When `check_finite` receives a scalar or slice containing a NaN or an infinity, it shall return `NotANumber` or `Infinite` with the index of the first offending element, and otherwise return the input unchanged.
- **REQ-KRN.NUM.ERR-003** When `probe_fp_env` runs, it shall return `FpEnvironment` if subnormal results are flushed to zero, subnormal inputs are treated as zero, or rounding is not round-to-nearest-even, using only ordinary f64 arithmetic and no `unsafe` or intrinsics.
- **REQ-KRN.NUM.ERR-004** The module shall contain no `unsafe`, no I/O, and no `unwrap`, `expect` or `panic` in non-test code.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/err-special-values-v1` (a table of f64 bit patterns: ±0, ±min subnormal, ±max, ±inf, quiet and signalling NaN payloads), when `check_finite` runs over each one alone and in a slice of 1 000 normal values at index 517, then infinities and NaNs return the right variant with index 517 and every other value passes unchanged (bit-equal).
- **AC-2** Given a normal IEEE-754 environment on x86-64, ARM64 and wasm32 (wasmtime), when `probe_fp_env` runs, then it returns `Ok(())` on all three.
- **AC-3** (error path) Given a test-only build on x86-64 that sets FTZ and DAZ through a test harness helper outside the kernel crate, when `probe_fp_env` runs, then it returns `FpEnvironment` naming the failed probe.
- **AC-4** Given the crate source, when the KNP-1 lint runs, then it reports no finding.

## Design

- **Registry command(s):** none; internal library module `fid_num::err`.
- **Data structures:** `enum NumError { NotANumber{op, index}, Infinite{op, index}, Domain{op, detail}, Singular{op, rank}, EmptyInput{op}, LengthMismatch{op, a, b}, FpEnvironment{probe} }`; `op` is a `&'static str`. Implements `Display` and `core::error::Error`.
- **Algorithm:** FTZ probe: `let t = f64::MIN_POSITIVE; (t * 0.5) != 0.0` must hold, computed through `core::hint::black_box` so it is not constant-folded. DAZ probe: a subnormal `s = f64::from_bits(1)` must satisfy `s + s == f64::from_bits(2)`. Rounding probe: `1.0 + 2^-53 == 1.0` (ties to even) and `1.0 + (2^-53 + 2^-78) > 1.0` and `-1.0 - (2^-53 + 2^-78) < -1.0` (not towards zero or downward).
- **Errors:** as listed; no other failure modes.
- **Determinism:** D0; integer and IEEE-exact operations only.
- **Performance budget:** `check_finite` on 1 M values < 2 ms on reference hardware [unverified until measured].
- **Crate:** `fid-num` (module `err`).

## Test fixtures

`fixtures/num/err-special-values-v1`: a hand-written table of bit patterns, licence CC-BY-4.0 (project data). The FTZ/DAZ helper for AC-3 lives in a test-only crate outside the kernel graph.

## Build prompt

> In the Rust crate `fid-num` (pure Rust, f64, no I/O, no `unsafe`, no dependencies), create module `err`. Define `pub enum NumError` with variants `NotANumber{op:&'static str,index:Option<usize>}`, `Infinite{op,index}`, `Domain{op,detail:&'static str}`, `Singular{op,rank:usize}`, `EmptyInput{op}`, `LengthMismatch{op,a:usize,b:usize}`, `FpEnvironment{probe:&'static str}`, implementing `Display` and `core::error::Error`. Add `pub fn check_finite(op, x: f64) -> Result<f64, NumError>` and `pub fn check_finite_slice(op, xs: &[f64]) -> Result<(), NumError>` returning the first offending index. Add `pub fn probe_fp_env() -> Result<(), NumError>` that detects flush-to-zero, denormals-are-zero and non-nearest-even rounding with ordinary arithmetic behind `core::hint::black_box` (no intrinsics). Tests: fixture `fixtures/num/err-special-values-v1` (bit-pattern table) for every special value; the probe returns Ok on x86-64, ARM64 and wasm32 under wasmtime; a test-only crate outside the kernel sets FTZ/DAZ and checks the probe fails. Constraints (KNP-1): no FMA or `mul_add`, no `f64::min`/`max`, no std HashMap/HashSet, no runtime SIMD dispatch, NaN is always an error; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable` in non-test code. Acceptance: AC-1..AC-4 of spec node KRN.NUM.ERR.

## Open questions

- None.

## Children

_Leaf: none._
