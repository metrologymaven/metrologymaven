---
id: UNC.RDL.GEN.HPREC
title: High-precision arithmetic, rounding and generator record
level: 4
parent: UNC.RDL.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.GEN-001, REQ-UNC.RDL.GEN-002]
covers: []
depends_on: [UNC.RDL.FORMAT, KRN.NUM.RNG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-rdl-gen]
updated: 2026-11-22
---

# UNC.RDL.GEN.HPREC: High-precision arithmetic, rounding and generator record

## Purpose

The shared foundation of every tier G generator: a high-precision real type (at least 60 decimal digits), correct rounding of coordinates to 16 significant decimal digits, a high-precision Gauss-Newton refinement that recomputes a reference solution for the rounded coordinates, the seeded random stream used to choose geometry, and the `GeneratorRecord` written into each dataset (method, tool version, seed, precision).

## Scope

**In:**
- `Hp` type wrapping a pure-Rust arbitrary-precision float at 200 bits (about 60 digits), with sqrt, sin, cos, atan2
- `round16(Hp) -> Decimal16`: round to 16 significant digits, ties to even, as a decimal string and its f64
- `hp_refine(residual_fn, p0, points) -> (p, cert)`: Gauss-Newton in `Hp` to a gradient norm below 1e-50 relative, with the final gradient norm as certificate
- Seeded geometry choice via the kernel counter-based RNG (seed, stream = family ordinal, index)
- `GeneratorRecord { family, method, tool_version, seed, precision_bits, digits }`
- Reference values written as decimal strings with at least 50 significant digits plus IEEE-754 hex

**Out:**
- Family-specific constructions (owned by LSNULL, MZEQ, GEOM, STRESS)
- Oracle cross-check (owned by `UNC.RDL.GEN.XCHECK`)

## Requirements

- **REQ-UNC.RDL.GEN.HPREC-001** `round16` shall return the decimal with 16 significant digits nearest to the input, ties to even, and the f64 nearest to that decimal.
- **REQ-UNC.RDL.GEN.HPREC-002** `hp_refine` shall stop only when the relative gradient norm is below 1e-50 and shall fail with `hp_not_converged` after 50 iterations.
- **REQ-UNC.RDL.GEN.HPREC-003** Reference outputs shall be written with at least 50 significant digits and the generator record, and the same family version and seed shall produce byte-identical records.
- **REQ-UNC.RDL.GEN.HPREC-004** The crate shall not depend on any kernel fit or solver crate.

## Acceptance criteria

- **AC-1** Given `fixtures/rdl/gen/hprec/round16.csv` (500 values including ties), then every `round16` result equals the expected decimal and f64.
- **AC-2** Given a circle residual and 20 points rounded with `round16`, when `hp_refine` runs from the pre-rounding solution, then the gradient certificate is below 1e-50 and the result agrees with an mpmath check in `fixtures/rdl/gen/hprec/circle_mp.txt` to 50 digits.
- **AC-3** Given the same seed twice on x86-64 and wasm32, then the generator record and outputs are byte-identical.
- **AC-4** Given the crate's dependency graph, then no `fid-krn-fit` or solver crate appears.

## Design

- **Registry command(s):** none.
- **Data structures:** `Hp`, `Decimal16`, `GeneratorRecord`, `HpCert { grad_norm, iterations }`.
- **Algorithm:** GN with QR in `Hp`; rounding by exact decimal conversion of the `Hp` mantissa.
- **Errors:** `hp_not_converged`, `hp_singular`.
- **Determinism:** D0 (integer-based arbitrary precision).
- **Performance budget:** a 1 000-point cylinder reference in under 10 s [unverified until measured].
- **Crate:** `fid-rdl-gen` (module `hp`).

## Test fixtures

`fixtures/rdl/gen/hprec/`: rounding table and an mpmath check file produced once by a committed script (CC-BY-4.0).

## Build prompt

> In crate `fid-rdl-gen`, module `hp`, implement: an `Hp` real type (200-bit pure-Rust arbitrary-precision float; licence on the project allowlist) with sqrt, sin, cos and atan2; `round16` (nearest 16-significant-digit decimal, ties to even, plus its nearest f64); `hp_refine` (Gauss-Newton with QR in `Hp`, stop at relative gradient norm < 1e-50, fail `hp_not_converged` after 50 iterations); a `GeneratorRecord` (family, method, tool version, seed, precision); geometry choice through the kernel counter-based RNG only. Write references as >= 50-digit decimal strings plus IEEE-754 hex. Do not depend on kernel fit or solver crates. Tests AC-1..AC-4. Constraints: CONSTITUTION 8 (no panic, no unwrap).

## Open questions

- Library choice and licence [unverified].

## Children

_Leaf: none._
