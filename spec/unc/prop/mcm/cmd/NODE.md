---
id: UNC.PROP.MCM.CMD
title: The unc.propagate_mcm@1 command and block scheduling
level: 4
parent: UNC.PROP.MCM
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.PROP.MCM-001, REQ-UNC.PROP.MCM-004]
covers: []
depends_on: [UNC.PROP.MCM.BLOCK, UNC.PROP.MCM.ADAPT, UNC.PROP.MCM.INTVL, UNC.PROP.MCM.SAMP, UNC.MODEL.BIND, CORE.BUS.PIPE, CORE.REG.DEF, CORE.JOB.PROG, CORE.STO.CAS]
side_effect: R1
determinism: D2
cli: [fid unc propagate-mcm]
mcp: [unc_propagate_mcm]
journal: [unc.propagate_mcm@1]
updated: 2026-11-17
---

# UNC.PROP.MCM.CMD: The unc.propagate_mcm@1 command and block scheduling

## Purpose

The registry command that runs adaptive Monte Carlo on the model bound to a result. It schedules blocks over worker threads, gathers them in block-index order, asks the adaptive rule after each completed prefix, enforces the failure-ratio limit, summarises the samples, stores the retained sample set in the blob store, and returns the scratch `McmResult`. Thread count never changes the result.

## Scope

**In:**
- `unc.propagate_mcm@1` (R1, long-running job with progress and cancellation)
- Scheduling: workers take block indices; results are consumed strictly in block-index order; speculative blocks beyond the stop point are discarded
- Failure-ratio verdict: fail with `trial_failures` above 0.1 % of attempted trials [unverified], naming the lowest failing trial index
- Storing `mcm-samples@1` bytes in the content-addressed store and returning the digest
- `McmResult { trials, interval, u, mc_mean, mean_minus_fx, samples_ref, flags, profile }`

**Out:**
- Engine functions (owned by BLOCK, ADAPT, INTVL, SAMP)
- Commit and method selection (owned by `UNC.PROP.SELECT`)

## Requirements

- **REQ-UNC.PROP.MCM.CMD-001** When `unc.propagate_mcm@1` runs, the system shall evaluate blocks until the adaptive rule returns stop or cap, considering only the completed prefix of blocks in index order.
- **REQ-UNC.PROP.MCM.CMD-002** Given the same model digest, inputs, seed and profile, the command shall return a bit-identical trial count, interval and sample-set digest on the same platform class for 1, 2 and 8 worker threads.
- **REQ-UNC.PROP.MCM.CMD-003** If failed trials exceed 0.1 % of attempted trials [unverified], then the command shall fail with `trial_failures` naming the lowest failing trial index and the count.
- **REQ-UNC.PROP.MCM.CMD-004** When cancelled, the command shall stop scheduling, return `cancelled` and store no sample set.
- **REQ-UNC.PROP.MCM.CMD-005** The command shall change no document state and write no Record Journal entry.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/select/ring.fidproj` result `C-2` (minimum-zone roundness), when `fid unc propagate-mcm --result C-2 --seed 7 --threads 1 --json` and the same with `--threads 8` run, then trial count, interval and samples digest are identical.
- **AC-2** Given `fixtures/unc/mcm/block/linear2.json` bound to a result, then the interval matches the analytic normal interval within the achieved tolerance.
- **AC-3** (error path) Given `fixtures/unc/mcm/cmd/fail_often.json` (about 1 % failing trials), then `trial_failures` with the lowest index.
- **AC-4** Given a cancel after the first block, then `cancelled` and the blob store holds no new sample set.
- **AC-5** Given a cap of 20 000 on a slowly converging model, then the result has flag `not_converged`.
- **AC-H1** Hypothesis: `unc.propagate_mcm@1` reproduces the published Monte Carlo estimate, standard uncertainty and coverage-interval endpoints of the JCGM 101:2008 worked examples (additive model, mass calibration, comparison loss, gauge-block calibration [unverified: clause 9 numbering]), encoded from their published input definitions with the published results as reference values, with agreement to the number of significant digits the guide publishes (at most one unit in the last published digit), measured by comparing the command's JSON output (seed recorded) with the published values; falsified if any example disagrees beyond that.
- **AC-H2** Hypothesis: Monte Carlo intervals from this command achieve coverage within the exact two-sided 99 % binomial band around p = 0.95 computed by `UNC.ORS.UNC.HARNESS` with N ≥ 1000 seeded repetitions on the ORS-UNC known-truth case `nonlin-rect` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record), measured by the harness coverage count; falsified if the count lies outside the band.

## Design

- **Registry command(s):** `unc.propagate_mcm@1` (R1; params `result_id`, `seed: u64`, `profile: String = "mcm-profile@1"` (p, n_dig, cap, gum_suite_profile), `threads: u32 = 0` (auto); returns `McmResult`).
- **Data structures:** `McmResult` with canonical encoding.
- **Algorithm:** resolve binding; validate; worker pool over block indices; ordered collector; ADAPT after each prefix block; INTVL on the final set; SAMP encode; CAS put.
- **Errors:** `model_missing`, `trial_failures{first, count}`, `cancelled`, engine errors typed.
- **Determinism:** D2.
- **Performance budget:** C-2 in under 30 s on 8 threads [unverified until measured].
- **Crate:** `fid-unc`.

## Test fixtures

`fixtures/unc/select/ring.fidproj` and `fixtures/unc/mcm/` (CC-BY-4.0).

## Build prompt

> In crate `fid-unc`, add registry command `unc.propagate_mcm@1` (R1; CLI `fid unc propagate-mcm`; MCP `unc_propagate_mcm`; `--json`; runs as a job with progress and cancellation). Resolve the bound model; run `fid_kunc::prop::mcm::block::run_block` on worker threads by block index; consume completed blocks strictly in index order and call `mcm_adapt` after each; discard speculative blocks past the stop. Fail `trial_failures` if failures exceed 0.1 % of attempted trials (name the lowest index). Summarise with `interval::summarize` (fx = the kernel value at the measured inputs), encode samples with `samples::encode`, and store them in the content-addressed store. Return `McmResult`. Results must be identical for any thread count. No Record Journal entry (R1). Tests AC-1..AC-5. Constraints: C1, CONSTITUTION 8.

## Open questions

- The 0.1 % failure limit is from the parent and unreviewed [unverified].

## Children

_Leaf: none._
