---
id: UNC.PROP.SELECT
title: Method selection, estimator policy and statement commit
level: 3
parent: UNC.PROP
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP-007, REQ-UNC.PROP-008]
covers: []
depends_on: [UNC.PROP.LPU, UNC.PROP.MCM, UNC.PROP.NLIN, UNC.PROP.VALID, UNC.PROP.RANK, UNC.UEC.CLASS, CORE]
side_effect: W2
determinism: D2
cli: [fid unc propagate-auto, fid unc propagate-commit, fid unc compare-methods]
mcp: [unc_propagate_auto, unc_propagate_commit, unc_compare_methods]
journal: [unc.propagate_auto@1, unc.propagate_commit@1, unc.compare_methods@1]
updated: 2026-11-16
---

# UNC.PROP.SELECT: Method selection, estimator policy and statement commit

## Purpose

The user-facing propagation commands. `unc.propagate_auto@1` picks LPU or MCM by a versioned rule, applies the versioned estimator policy and returns a scratch uncertainty statement; `unc.compare_methods@1` runs both and validates LPU against MCM; `unc.propagate_commit@1` attaches a statement to its result as a journaled W2 change. The rule and policy versions are recorded in every statement.

## Scope

**In:**
- Selection rule `unc-select@1` and its data table `nonsmooth-ops@1` (operator algorithm-ID patterns that are non-smooth)
- Estimator policy `est-policy@1`: reported value is the kernel value at the measured inputs; bias disclosure for non-negative quantity kinds near zero
- Commands `unc.propagate_auto@1` (R1), `unc.compare_methods@1` (R1), `unc.propagate_commit@1` (W2)
- Building the statement (class from `UNC.UEC.CLASS`, contributors from `UNC.PROP.RANK`)

**Out:**
- The engines (owned by `UNC.PROP.LPU`, `UNC.PROP.MCM`, `UNC.PROP.MVAR`)
- The validation computation (owned by `UNC.PROP.VALID`)

## Requirements

- **REQ-UNC.PROP.SELECT-001** When `unc.propagate_auto@1` runs, the system shall select MCM if any operator on the path to the output matches a pattern in `nonsmooth-ops@1` (minimum-zone, minimum-circumscribed and maximum-inscribed fits, max/min reductions, position with bonus) or if NLIN's outcome is not `linear_adequate`, and LPU otherwise.
- **REQ-UNC.PROP.SELECT-002** Every statement produced shall record `selection_rule = "unc-select@1"` and `estimate_convention = "est-policy@1"`.
- **REQ-UNC.PROP.SELECT-003** The reported value attached with a statement shall be the kernel value at the measured inputs and shall never be the MC mean.
- **REQ-UNC.PROP.SELECT-004** When the output quantity kind is non-negative (form, runout, position deviation) and the reported value is below 3 times its standard uncertainty [unverified], the statement shall disclose the MCM `mean_minus_fx` bias value and mark `bias_disclosed`.
- **REQ-UNC.PROP.SELECT-005** When `unc.compare_methods@1` runs, the system shall run LPU and MCM on the same model, inputs and seed, call `unc.prop.validate_lpu@1.0.0`, and return both results with `lpu_validated_by_mc`.
- **REQ-UNC.PROP.SELECT-006** When `unc.propagate_commit@1` runs on a scratch statement, the system shall verify that its model digest, input digests and seed match the result's current inputs and attach it in one journal event, and if they do not match, then it shall fail with `stale_statement` and commit nothing.
- **REQ-UNC.PROP.SELECT-007** If the result has no bound model, then `unc.propagate_auto@1` shall return a UEC-0 statement with reason from `UNC.UEC.CLASS` and shall not fail.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/select/ring.fidproj` (an LS circle diameter `C-1` and a minimum-zone roundness `C-2`, both with bound models), when `fid unc propagate-auto --result C-1 --seed 7 --json` runs, then the method is `LPU`; for `C-2` it is `MCM`; both statements carry `unc-select@1` and `est-policy@1`.
- **AC-2** Given `C-2` with true roundness near zero in the fixture, when propagated, then the reported value equals the kernel roundness value bit for bit, and `bias_disclosed = true` with `mean_minus_fx > 0`.
- **AC-3** Given `C-1`, when `fid unc compare-methods --result C-1 --seed 7 --json` runs, then `lpu_validated_by_mc = true`.
- **AC-4** Given a scratch statement from AC-1, when `fid unc propagate-commit --result C-1 --statement <digest>` runs, then the result carries the statement and one `unc.propagate_commit@1` event is journaled.
- **AC-5** (error path) Given AC-4's scratch statement after the result's input points changed, when committed, then it fails with `stale_statement` and the journal head is unchanged.
- **AC-6** (UEC-0) Given a result `C-3` with no model, when propagated, then a UEC-0 statement with reason `model_missing` is returned.
- **AC-7** (replay) Given the journal from AC-1..4, when replayed in exact mode on the same platform class, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.propagate_auto@1` (R1; `result_id`, `seed: u64`, `profile`; returns scratch statement plus engine outputs), `unc.compare_methods@1` (R1; same params; returns both results and validation), `unc.propagate_commit@1` (W2; `result_id`, `statement_digest`; returns new result digest).
- **Data structures:** `nonsmooth-ops@1` as a versioned data file of algorithm-ID glob patterns; `EstPolicy { version, nonneg_kinds: Vec<QuantityKind>, disclosure_ratio: Decimal = 3 }`.
- **Algorithm:** resolve bound model; walk operator path (sorted) against patterns; consult NLIN when LPU is a candidate; run the selected engine through the kernel; build the statement with class from `uec::assign` and contributors from RANK; scratch statements are cached by digest for commit.
- **Errors:** `stale_statement`, `result_not_found`, engine errors passed through typed.
- **Determinism:** D2 (MCM seeded); LPU paths are D0.
- **Performance budget:** dispatch overhead under 1 ms beyond the engine's time [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

`fixtures/unc/select/ring.fidproj`: synthetic ring with 360 points generated from a known circle plus seeded noise (seed `0x52494E47`), with bound models; built by a test helper (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, implement three registry commands: `unc.propagate_auto@1` (R1; CLI `fid unc propagate-auto`; MCP `unc_propagate_auto`), `unc.compare_methods@1` (R1; `fid unc compare-methods`; `unc_compare_methods`) and `unc.propagate_commit@1` (W2; `fid unc propagate-commit`; `unc_propagate_commit`). Each is journaled with its command ID and result digest.
> Selection rule `unc-select@1`: choose MCM if any operator on the model path matches the versioned glob table `nonsmooth-ops@1` (minimum-zone, minimum-circumscribed, maximum-inscribed fits; max/min reductions; position with bonus) or the NLIN outcome is not `linear_adequate`; else LPU. Estimator policy `est-policy@1`: the reported value is the kernel value at the measured inputs, never the MC mean; for non-negative kinds (form, runout, position deviation) with value < 3·u, set `bias_disclosed` and copy the MCM output `mean_minus_fx`. Record both versions in every statement. Class comes from `fid-kunc` `uec::assign`; contributors from `prop::rank`.
> compare_methods: run LPU and MCM with the same seed, call `prop::validate_lpu` (`unc.prop.validate_lpu@1.0.0`), return both and the flag. propagate_commit: verify the scratch statement's model digest, input digests and seed against the result's current state, else `stale_statement` with nothing committed. No bound model: return UEC-0 (no error).
> Tests with `fixtures/unc/select/ring.fidproj`: LPU for the LS diameter, MCM for minimum-zone roundness; reported value bit-equal to the kernel value; bias disclosed near zero; LPU validated for the diameter; commit journals one event; stale commit refused; UEC-0 for the unbound result; exact replay IDENTICAL. Values of record are computed only in the kernel (CONSTITUTION C1). No panics, no `unwrap`.

## Open questions

- The disclosure threshold (value < 3u) is a placeholder for review by an uncertainty expert [unverified].
- Bias correction methods (rather than disclosure) are future `est-policy` versions; GUM-11 Bayesian estimation is the tracked option.

## Children

_Leaf: none._
