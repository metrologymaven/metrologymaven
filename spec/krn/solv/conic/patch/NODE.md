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
satisfies: [REQ-KRN.SOLV.CONIC-002]
covers: []
depends_on: [KRN.NUM.FN, KRN.NUM.RED, KRN.NUM.ERR]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
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
- Which third-party crates upstream depends on and whether each can be removed [unverified].

## Children

_Leaf: none._
