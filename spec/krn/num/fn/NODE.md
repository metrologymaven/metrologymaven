---
id: KRN.NUM.FN
title: Transcendentals and total-order operations
level: 3
parent: KRN.NUM
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-num]
satisfies: [REQ-KRN.NUM-001, REQ-KRN.NUM-002, REQ-KRN.NUM-003, REQ-KRN.NUM-007, REQ-KRN.NUM-010]
covers: []
depends_on: [KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.NUM.FN: Transcendentals and total-order operations

## Purpose

The facade module through which the kernel gets every elementary function and every min, max and clamp. It vendors one pinned, pure-Rust libm implementation so results are bit-identical on x86-64, ARM64 and wasm32, adds total-order comparisons with defined signed-zero behaviour, and ships the self-test whose digest `KRN.MAN` records.

## Scope

**In:** sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, ln, hypot, cbrt; `total_min`, `total_max`, `total_clamp`, `total_cmp`; domain checks; the cross-target self-test.
**Out:** reductions (`KRN.NUM.RED`); linear algebra (`KRN.NUM.LA`); the lint that bans platform libm (`KRN.NUM.LINT`); recording the self-test digest in the manifest (`KRN.MAN`).

## Requirements

- **REQ-KRN.NUM.FN-001** The module shall provide sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, ln, hypot and cbrt, each as a function `fn(f64...) -> Result<f64, NumError>`.
- **REQ-KRN.NUM.FN-002** Every function except sqrt shall be computed by one vendored, version-pinned pure-Rust libm source tree inside the kernel repository, and sqrt shall use the IEEE-754 correctly rounded square root.
- **REQ-KRN.NUM.FN-003** If an argument is outside a function's real domain (for example asin(1.5), ln(-1), sqrt(-1)), then the function shall return `NumError::Domain` and never a NaN.
- **REQ-KRN.NUM.FN-004** `total_min` and `total_max` shall order -0.0 below +0.0, shall return `NumError::NotANumber` if either operand is NaN, and `total_clamp` shall return an error if lo > hi.
- **REQ-KRN.NUM.FN-005** The module shall provide a self-test that evaluates every function on 10 000 [unverified] fixed inputs per function and exposes the output bit patterns in a fixed order, and the SHA-256 of those outputs shall be identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.NUM.FN-006** Each vendored function shall stay within 1 ulp [unverified] of a high-precision oracle on the self-test inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/num/fn-selftest-v1` (input generator definition plus golden SHA-256 per function), when the self-test runs on x86_64-unknown-linux-gnu, aarch64-apple-darwin, x86_64-pc-windows-msvc and wasm32-unknown-unknown under wasmtime, then every function's digest equals the golden digest on every target.
- **AC-2** Given the same inputs and the oracle table `fixtures/num/fn-oracle-v1` (values computed offline at 200-bit precision), when compared, then every output is within 1 ulp [unverified] and the maximum ulp error per function is printed.
- **AC-3** (error path) Given asin(1.0000000000000002), ln(-0.5), sqrt(-1e-300) and atan2(NaN, 1), when called, then each returns the typed error (`Domain` or `NotANumber`) and no value.
- **AC-4** Given (+0.0, -0.0) in both orders, when `total_min` and `total_max` run, then min returns -0.0 and max returns +0.0 (checked by bit pattern), and either operand NaN returns `NotANumber`.

## Design

- **Registry command(s):** none; module `fid_num::f`.
- **Data structures:** free functions; `pub const SELFTEST_INPUTS_PER_FN: usize = 10_000`; `pub fn selftest_outputs(f: FnId) -> impl Iterator<Item = u64>`.
- **Algorithm:** vendor the source of a musl-derived pure-Rust libm (candidate: the `libm` crate from the rust-lang organisation, MIT OR Apache-2.0 [unverified]) as `fid-num/src/f/vendor/` with its upstream commit recorded in `VENDOR.md`. Remove any use of `mul_add`, inline assembly and target-specific code paths so that only the generic path remains. Self-test inputs come from a splitmix64 sequence over bit patterns, mapped into each function's domain plus edge values (0, ±tiny, ±huge, multiples of pi/2 near the reduction boundaries).
- **Errors:** `NumError::{Domain, NotANumber, Infinite}`.
- **Determinism:** D0; the generic code path has no platform dependence once FMA, intrinsics and assembly are removed.
- **Performance budget:** within 3x of platform libm per call [unverified until measured].
- **Crate:** `fid-num` (module `f`).

## Test fixtures

`fixtures/num/fn-selftest-v1`: the generator definition (seed and domain maps) plus golden digests, CC-BY-4.0. `fixtures/num/fn-oracle-v1`: oracle values computed offline with an arbitrary-precision library outside the kernel (the generator script is committed), CC-BY-4.0.

## Build prompt

> In crate `fid-num`, create module `f`. Vendor a pinned musl-derived pure-Rust libm source tree (record upstream URL, commit and licence in `src/f/vendor/VENDOR.md`; its licence must be on the project allowlist, MIT or Apache-2.0) and strip every `mul_add`, inline assembly, `cfg(target_arch)` fast path and intrinsic so only the generic path remains. Expose `sqrt, sin, cos, tan, asin, acos, atan, atan2, exp, ln, hypot, cbrt` as `pub fn ... -> Result<f64, NumError>` (use `NumError` from module `err`); return `Domain` for out-of-domain arguments and `NotANumber`/`Infinite` for non-finite inputs. sqrt uses `f64::sqrt` (IEEE correctly rounded). Add `total_cmp`, `total_min`, `total_max` (-0.0 < +0.0, NaN -> error) and `total_clamp` (error if lo > hi). Add `selftest_outputs(FnId) -> impl Iterator<Item=u64>` over 10 000 deterministic inputs per function (splitmix64 over bit patterns, mapped into each domain, plus listed edge values). Tests: golden SHA-256 per function from `fixtures/num/fn-selftest-v1` (sha2 as dev-dependency only) on Linux x86-64, macOS ARM64, Windows x86-64 and wasm32 under wasmtime; ulp error vs `fixtures/num/fn-oracle-v1` <= 1. KNP-1: no FMA, no `powi`, no platform libm (`f64::sin` etc. banned), no `f64::min`/`max`, no HashMap/HashSet, no runtime SIMD dispatch, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-4 of spec node KRN.NUM.FN.

## Open questions

- Licence and maintenance status of the candidate libm source must be confirmed against the allowlist that ECO.RULES writes under D-001 [unverified].

## Children

_Leaf: none._
