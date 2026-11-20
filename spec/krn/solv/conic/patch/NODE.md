---
id: KRN.SOLV.CONIC.PATCH
title: KNP-1 patches to the vendored interior-point solver
level: 4
parent: KRN.SOLV.CONIC
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [vendor/fid-clarabel]
built_pr: 305
satisfies: [REQ-KRN.SOLV.CONIC-002]
covers: []
depends_on: [KRN.NUM.FN, KRN.NUM.RED, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# KRN.SOLV.CONIC.PATCH: KNP-1 patches to the vendored interior-point solver

## Purpose

Turns the pinned upstream interior-point solver (a fork of Clarabel, `fid-clarabel`) into kernel code: f64 only, single-threaded, no I/O or clocks, every transcendental and min/max through `fid-num`, no hash maps, and a deterministic sparse LDL factorization path. It also strips the cones and features the kernel does not use, so less code needs auditing. Each change is a numbered patch with a rationale, recorded in the audit record.

## Scope

**In:**
- Scope-reduction patches: keep zero, nonnegative and second-order cones and the QP objective; remove exponential, power and PSD cones, language bindings, printing, timers, settings parsing from files, and optional parallel features
- Specialise the generic float type to f64
- Replace float transcendentals (`sqrt`, `powi`, `powf`, `ln`, `exp`) with `fid_num::fn` calls and `f64::min`/`max` with `fid_num::fn::total_min`/`total_max`
- Replace `HashMap`/`HashSet` with ordered maps or vectors
- Route norms and dot products in the iteration through `fid_num::red`; fix the order of every accumulation loop
- Deterministic LDL path: fixed fill-reducing ordering computed from the sparsity pattern only, fixed static regularisation and iterative refinement counts, no FMA
- Reducing third-party dependencies to those on the kernel allowlist or vendoring them under the same audit

**Out** (and which node owns it instead):
- Layout, digest and audit check (`KRN.SOLV.CONIC.VEND`)
- The kernel-facing API, limits and errors (`KRN.SOLV.CONIC.WRAP`)
- Cross-target test set (`KRN.SOLV.CONIC.XTEST`)

## Requirements

- **REQ-KRN.SOLV.CONIC.PATCH-001** After patching, the KNP-1 lint over the patched crate shall report no unwaived finding, and every waiver shall be listed with its justification in `AUDIT.toml`.
- **REQ-KRN.SOLV.CONIC.PATCH-002** The patched crate shall support only zero, nonnegative and second-order cones with an optional positive semidefinite quadratic objective, and shall contain no exponential, power or PSD cone code.
- **REQ-KRN.SOLV.CONIC.PATCH-003** The patched crate shall have no thread spawning, no clock reads, no printing or file access, and no feature flag that re-enables them.
- **REQ-KRN.SOLV.CONIC.PATCH-004** The LDL factorization shall compute its ordering from the sparsity pattern alone with a fixed tie-break by lowest index, and shall use fixed regularisation and refinement counts, so the factorization path depends only on the problem data.
- **REQ-KRN.SOLV.CONIC.PATCH-005** Each patch shall be one file `patches/NNNN-<slug>.patch` changing one concern, with its KNP-1 rule IDs or `scope-reduction` and its rationale in `AUDIT.toml`.
- **REQ-KRN.SOLV.CONIC.PATCH-006** On upstream's own example problems that remain in scope, the patched solver's objective values shall agree with unpatched upstream within 1e-8 relative [unverified tolerance].

## Acceptance criteria

- **AC-1** Given the patched `vendor/fid-clarabel/crate/`, when `fid-knp1-lint check` runs with the kernel config, then there are no unwaived findings in the crate.
- **AC-2** Given the patched crate source, when searched for `exp_cone`, `pow_cone`, `psd`, `std::time`, `std::thread`, `println!` and `HashMap`, then there are no matches outside comments.
- **AC-3** Given `fixtures/solv/patch/upstream-examples-v1` (the in-scope upstream examples with objectives recorded from unpatched upstream on x86-64), when the patched solver runs, then every objective agrees within 1e-8 relative.
- **AC-4** Given one example problem, when solved twice with the rows of an equality block presented in the same order, then all output bits are identical; and the LDL permutation is the same on x86-64, ARM64 and wasm32.
- **AC-5** Given `cargo xtask conic-audit`, then it passes with this leaf's patches.

## Design

- **Registry command(s):** none; vendored library crate `fid-clarabel` (internal to `fid-solv`).
- **Data structures:** unchanged upstream structures specialised to f64; ordered maps (`BTreeMap`) or index vectors in place of hash maps.
- **Algorithm:** mechanical patch series in this order: 0001 scope reduction (cones, bindings, printing, timers), 0002 f64 specialisation, 0003 transcendentals and min/max through `fid-num`, 0004 hash maps, 0005 reductions through `fid_num::red`, 0006 LDL ordering and regularisation fixes, 0007 dependency trimming. Reference: Goulart and Chen, Clarabel: An interior-point solver for conic programs with quadratic objectives, arXiv:2405.12762 (2024) [unverified citation details].
- **Errors:** upstream status codes are kept; `KRN.SOLV.CONIC.WRAP` maps them.
- **Determinism:** D0 is the target, proven by `KRN.SOLV.CONIC.XTEST`.
- **Performance budget:** within 1.5x of unpatched upstream on the in-scope examples [unverified until measured].
- **Crate:** `fid-clarabel` (vendored, kernel layer L0).

## Test fixtures

`fixtures/solv/patch/upstream-examples-v1`: the upstream project's example problems that stay in scope, with objectives recorded from the unpatched build; upstream licence (Apache-2.0 [unverified]) noted in `ATTRIBUTION.md`.

## Build prompt

> In `vendor/fid-clarabel/`, starting from the pinned pristine `upstream/` tree, write a patch series into `patches/` and produce `crate/`: (0001) remove exponential, power and PSD cones, language bindings, printing, timers and parallel features; (0002) specialise the float type to f64; (0003) replace `sqrt`, `powi`, `powf`, `ln`, `exp` with `fid_num::fn` and `f64::min/max` with `fid_num::fn::total_min/total_max`; (0004) replace HashMap/HashSet with BTreeMap or vectors; (0005) route norms and dots through `fid_num::red`, fixing loop order; (0006) make the LDL ordering a function of the sparsity pattern only with lowest-index tie-breaks and fix regularisation and refinement counts; (0007) drop or vendor third-party dependencies not on `kernel-allowlist.toml`. Record each patch in `AUDIT.toml` with KNP-1 rule IDs or `scope-reduction` and a rationale. Tests: `fid-knp1-lint check` clean on `crate/`; forbidden-symbol search; `fixtures/solv/patch/upstream-examples-v1` objectives within 1e-8 relative; repeated-run bit identity; `cargo xtask conic-audit` passes. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, no `unsafe`, no I/O, deny `unwrap`/`expect`/`panic` in non-test code (convert upstream panics to error statuses). Acceptance: AC-1..AC-5 of spec node KRN.SOLV.CONIC.PATCH.

## Open questions

- The parent asked whether the sparse LDL can be made deterministic by patching or must be replaced by `fid-num` dense factorizations. This leaf patches it (the LDL is a sequential algorithm, so fixed ordering, fixed regularisation and no FMA should suffice [unverified]). If `KRN.SOLV.CONIC.XTEST` shows cross-target divergence, replacing the LDL would need an LDL^T in `KRN.NUM.LA`, which that module does not offer today; raise it then as a change to KRN.NUM.LA.
- Upstream may panic on some inputs (index checks, `unwrap`); converting every panic path is part of patch 0003 or a separate patch [unverified count].
- Which third-party crates upstream depends on and whether each can be removed: resolved in the build. Clarabel 0.11.1 requires amd, cfg-if, derive_builder, enum_dispatch, itertools, lazy_static, num-traits and thiserror (plus optional serde, BLAS/LAPACK, faer, Pardiso, pyo3 and others behind features). Patches 0001, 0002, 0006 and 0007 remove all of them; `fid-num` is the only dependency, so `kernel-allowlist.toml` stays empty.
- **As built (vendor/fid-clarabel/, fid-clarabel 0.11.1):** the series has nine patches, not seven. 0008 makes every file classifiable by the KNP-1 syntax scan, which fails closed on non-ASCII identifiers (Clarabel's Greek names), two-segment `use super::Name` imports, macro metavariables in callee position and `#[default]`; its `AUDIT.toml` entry can only name `KNP1-ENV` and `KNP1-SIMD`, because the VEND schema accepts no `KNP1-PARSE` rule ID. 0009 adds the Apache-2.0 section 4(b) change notices (reason class `licence-notice`, added to the VEND schema by maintainer decision); `tests/kernel_source.rs` walks `crate/` against `upstream/` and fails if a differing or new file lacks the notice in its first five lines (with a negative test).
- **As built, f64 specialisation (0002):** `T` stays as the name of the float type but is an alias of `f64` (`pub type T = f64`); the generic parameters are removed. This keeps the later patches line-comparable with upstream.
- **As built, NaN and errors (0003, 0005):** `fid_num::f` and `fid_num::red` return errors; the solver's numeric code is infallible, so `algebra/knp.rs` and `vecmath.rs` map an error to NaN, and the solver ends with `NumericalError`. On a non-finite μ the solver restores the previous iterate (recomputing its residuals and info) before the "almost solved" check, and at iteration 0 the info keeps the NaN values `reset()` gives every scalar, so no check passes. A solved, almost solved or infeasible status is demoted to `NumericalError` unless x, z and s (and, for a solution, the objectives) are finite. `DefaultSolver::new` rejects malformed CSC data, NaN, infinite P, A or q, -infinity in b and cone sizes that overflow `usize` with `BadInputData`; `update_P/A/q/b` reject any non-finite value with `DataUpdateError::NonFiniteData` before changing anything (stricter than setup for b: updates run without presolve, which is what removes +infinity bounds). Test: `tests/kernel_errors.rs` forces the non-finite-μ path (q = 1e308) and sweeps extreme scales; no forced case reaches the restore branch (iteration > 0) through the public API, so that branch is covered by review only.
- **As built, panics:** every `panic!`, `unwrap`, `expect`, `unreachable!` and `debug_assert!` is gone, and the public `CscMatrix` entry points no longer assert: `new` does not check (the solver validates with `check_format`), `From` of ragged rows gives a matrix that fails `check_format`, `new_from_triplets` and `set_entry` return `Result`, `get_entry` and `index_to_coord` return `None` out of range; `QDLDLFactorisation::solve` fills `b` with NaN instead of asserting. The remaining `assert!`/`assert_eq!` checks (and slice indexing) are preconditions on solver-built buffers; each group carries a `// invariant (patch 0003):` comment: `CscMatrix::select_rows` (presolve keep flags, one per row of A), `CscMatrix::to_triu` (P is square by `check_dimensions`), the `MatrixMath`/`MatrixVectorMultiply`/`SymMatrixMath` helpers in `csc/matrix_math.rs` (work vectors sized from the matrix; matrices pass `check_format`), `colcount_missing_diag` (KKT assembly allocates every counted block), `invperm`/`sortperm*` (solver-built permutations), the `VectorMath` helpers in `vecmath.rs` (same-dimension solver vectors or cone ranges), the nonnegative-cone methods (slices are the cone's `rng_cones`/`rng_blocks` range) and `SecondOrderCone::new` (`new_collapsed` drops SOC(0) and folds SOC(1) into the nonnegative cone). The asserting math traits (`ScalarMath`, `VectorMath`, `MatrixMath`, `MatrixMathMut`, and the already crate-private `MatrixVectorMultiply`/`SymMatrixMath` with symv and gemv) and `CscMatrix::select_rows`/`to_triu` are `pub(crate)`, so no caller outside the crate reaches these asserts; the public algebra is the `CscMatrix` constructors, `identity`, `zeros`, `negate`, `vcat` and the checked accessors. The ported upstream tests carry their own `dist`/`negate`/`scale`/`to_triu` helpers instead.
- **As built, LDL ordering budget (0006):** the exact minimum-degree ordering scans every node per pivot (O(n^2) plus the fill), so it refuses more than `MAX_ORDERING_DIM` = 20 000 columns, `MAX_ORDERING_NNZ` = 1 000 000 stored entries or an elimination graph above `MAX_ORDERING_FILL` = 4 000 000 edges with `QDLDLError::OrderingTooLarge` (a malformed pattern with `QDLDLError::InvalidPattern`); `DefaultSolver::new` refuses a problem whose KKT bound exceeds the first two as `BadInputData` (the fill budget can only fail while ordering, which gives `NumericalError`). Measured locally (x86-64 Windows, not a CI timing): the example_box KKT pattern takes 20 ms at dimension 6 000 and 0.2 s at 19 998 in release (0.5 s and 5.8 s in a debug build). `tests/kernel_errors.rs` orders and solves the box problem at n = 2000 (dimension 6 000) without timing it.
- **As built, AC-1 (follow-up: KRN.NUM.LINT.CLI):** `fid-knp1-lint check` (KRN.NUM.LINT.CLI) does not exist yet, so `vendor/fid-clarabel/knp1-findings.json` is written by `vendor/fid-clarabel/lint-driver/`, which runs the built syntax scan (KRN.NUM.LINT.SYN) over `crate/src/` and adds the `linted` entry: no finding, two facade notes. The syntax scan reads no lockfile, so the report records the digest of empty lockfile input and depends only on the source and `knp1-lint.toml`. CI runs the driver with `--check` before `cargo xtask conic-audit`, so a stale report fails. **Follow-up:** when KRN.NUM.LINT.CLI is built, replace the driver with `fid-knp1-lint check` (graph, flags, typed and data modules included), regenerate the report and update `lint_report_sha256`; AC-1 is fully met only then.
- **As built, scope:** `set_infinity`/`default_infinity` (a mutable global) are removed; the infinity bound is the constant 1e20. Upstream's unit tests and doc examples are dropped from the crate (doctest = false); upstream's in-scope integration tests (basic_lp, basic_qp, basic_socp, basic_eq_constrained, basic_unconstrained, presolve, data_updating, api_dimension_checks; 41 tests with upstream's statuses, including the infeasible, unbounded and degenerate cases) are ported to `vendor/fid-clarabel/tests/upstream_tests.rs`, where the kernel tests also live so that `crate/` stays upstream plus patches.
- **As built, AC-3:** the patched objectives agree with unpatched upstream within about 2e-15 relative, far inside 1e-8. Performance against upstream was not measured [unverified].

## Children

_Leaf: none._
