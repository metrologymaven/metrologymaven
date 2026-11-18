---
id: KRN.NUM
title: fid-num numerics facade and KNP-1 enforcement
level: 2
parent: KRN
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-KRN-001, REQ-KRN-005]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.NUM: fid-num numerics facade and KNP-1 enforcement

## Purpose

The bottom of the kernel graph. `fid-num` is the only place where the kernel touches floating-point primitives that could differ between hosts: transcendentals, min/max, reductions, dense linear algebra and random numbers. The KNP-1 lints make sure nothing in the kernel graph goes around it. Every other KRN subsystem builds on this one, so it is separate and comes first.

## Scope

**In:**
- Transcendental functions through a vendored, pinned implementation behind the facade (no platform libm)
- Total-order min, max and clamp, including signed-zero handling
- Fixed-order reductions (pairwise or compensated sums over fixed partitions)
- Deterministic dense linear algebra: QR with a fixed pivoting path, and SVD and symmetric eigen decomposition, with rank and condition-number reporting
- Seeded, counter-based RNG (seed, stream and index addressing)
- NaN and FP-environment checks at kernel entry, mapped to typed errors
- The KNP-1 lint set (no FMA or contraction, no `powi`, no `f64::min`/`max`, no std HashMap/HashSet, no runtime SIMD dispatch, no `unsafe`, no I/O), applied transitively over the whole kernel dependency graph
- The kernel dependency allowlist check (the SPDX allowlist itself comes from D-001)
- The exact decimal comparator `kdec` and the versioned kernel number formatters `fmt.*@version` (owner assigned at the level-2 review, 2026-11-16)

**Out** (and which node owns it instead):
- LP, QP and SOCP solvers and nonlinear least-squares drivers (`KRN.SOLV`)
- The kernel manifest, WASM build and cross-host bit-identity gate (`KRN.MAN`)
- Geometry types and spatial indexes (`GEO`)
- Forward-mode autodiff for uncertainty (`UNC`, if it needs it; may later move here by decision)

## Requirements

- **REQ-KRN.NUM-001** The `fid-num` crate shall be pure Rust on f64 with no I/O, no `unsafe` and no dependency outside the kernel allowlist.
- **REQ-KRN.NUM-002** The facade shall provide every transcendental function the kernel uses (at least sqrt, sin, cos, tan, atan2, asin, acos, exp, ln, hypot and cbrt) from one vendored implementation whose outputs are bit-identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.NUM-003** The facade shall provide total-order min, max and clamp whose result for (+0, -0) operands is defined and documented, and which return a typed error on any NaN operand.
- **REQ-KRN.NUM-004** The facade shall provide summation and dot-product reductions whose result depends only on the input order and length, never on thread count or target.
- **REQ-KRN.NUM-005** The facade shall provide QR, SVD and symmetric eigen decompositions that report numerical rank and a condition-number estimate with every factorization.
- **REQ-KRN.NUM-006** The facade shall provide a counter-based RNG addressed by (seed, stream, index) whose output sequence is identical on every target for the same address.
- **REQ-KRN.NUM-007** If any facade operation produces or receives a NaN or infinity where a finite value is required, then it shall return a typed numeric error and never propagate the value.
- **REQ-KRN.NUM-008** When CI runs, the KNP-1 lint shall scan the whole kernel dependency graph and fail the build on any banned API (FMA, `mul_add`, `powi`, platform libm, `f64::min`/`max`, std HashMap/HashSet, runtime CPU-feature dispatch, `unsafe`, file, network, clock or environment access).
- **REQ-KRN.NUM-009** The KNP-1 lint shall fail the build on any kernel source reference to a reference-dataset ID, file name or embedded reference constant.
- **REQ-KRN.NUM-010** The facade shall ship a self-test of at least 10 000 [unverified] fixed inputs per function whose output digest is identical on x86-64, ARM64 and wasm32.
- **REQ-KRN.NUM-011** The facade shall provide an exact decimal comparator (`kdec`) that orders any finite f64 against any decimal limit by exact rational comparison, without converting the limit to f64.
- **REQ-KRN.NUM-012** The facade shall provide versioned number formatters `fmt.<kind>@<major>` that turn an SI f64 value into a decimal string in a requested unit, and that never print a value lying on the nonconforming side of a limit as equal to that limit.
- **REQ-KRN.NUM-013** The kernel shall provide exact-sign geometric predicates (orientation and in-circle/in-sphere on f64 inputs, with exact fallback and no fused multiply-add) as one shared module, so that mesh cutting, repair, meshing and Voronoi-based fits do not each implement their own (D-035).

## Interfaces

- **Offers:** the `fid-num` crate API (transcendentals, total-order ops, reductions, dense LA, RNG, numeric error type, `kdec`, `fmt.*@version`); the `knp1-lint` CI check; a facade self-test digest consumed by `KRN.MAN`.
- **Consumes:** nothing inside the platform.

## Decomposition hints

FN (transcendentals and total-order ops), RED (reductions), LA (QR, SVD, eigen), RNG, ERR (numeric error types and FP-environment checks), LINT (KNP-1 lint, transitive dependency scan, dataset-constant ban).

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** KRN.NUM owns the kernel number formatters `fmt.*@version` and the exact decimal comparator (`kdec`). They compare f64 values to decimal limits exactly, and never print a nonconforming value as equal to a limit (RPT REQ-RPT-007; baseline principle P2). **Incorporated** at this split as REQ-KRN.NUM-011 and -012, owned by `KRN.NUM.KDEC` and `KRN.NUM.FMT`. RPT's open question (formatters not yet named) is answered by those two nodes.
- Which vendored libm source (the concept doc names a musl-derived crate) and its licence fit D-001 [unverified].
- Whether contraction can be disabled reliably for every target through compiler flags alone, or needs source-level checks too [unverified].

## Split rationale

The split follows the **kinds of numeric primitive** that can differ between hosts, plus enforcement. Each child is one module of `fid-num` (or, for LINT, one CI tool), so leaves stay in one crate and are reviewable on their own: elementary functions (FN), reductions (RED), dense linear algebra (LA), random numbers (RNG), error and FP-environment checks (ERR), lint enforcement (LINT), and the decimal pair added by the level-2 review: the exact comparator (KDEC) and the formatters (FMT). FMT depends on KDEC because both need the same exact dyadic-versus-decimal arithmetic, and KDEC is the part Track A needs first.

REQ-KRN.NUM-011 and -012 were added at this split to record the owner assignment accepted at the level-2 review; no existing requirement was changed.

Alternatives considered:
- **Put `kdec` and `fmt.*` in a separate KRN subsystem.** Rejected: the review assigned them here, and they share the facade's error type and exactness rules.
- **One child for all of linear algebra and reductions.** Rejected: reductions are small and needed by every other child; LA is the largest and riskiest piece and will split again.
- **Fold ERR into FN.** Rejected: every child returns the same error type, so it must sit below all of them.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| KRN.NUM.ERR | Numeric error type and FP-environment checks | 001 007 | none |
| KRN.NUM.FN | Transcendentals and total-order operations | 001 002 003 007 010 | none |
| KRN.NUM.RED | Fixed-order reductions | 001 004 007 | none |
| KRN.NUM.LA | Deterministic dense linear algebra | 001 005 007 | none |
| KRN.NUM.RNG | Counter-based random number generator | 001 006 | none |
| KRN.NUM.LINT | KNP-1 lint and kernel dependency allowlist check | 001 008 009 | none |
| KRN.NUM.KDEC | Exact decimal type and comparator (kdec) | 001 007 011 | none |
| KRN.NUM.FMT | Versioned kernel number formatters (fmt.*) | 001 007 012 | none |
| KRN.NUM.PRED | Exact geometric predicates | 013 | none |
