---
id: UNC.PROP.MCM.BLOCK
title: Trial-block sampling and per-trial kernel evaluation
level: 4
parent: UNC.PROP.MCM
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 338
satisfies: [REQ-UNC.PROP.MCM-001, REQ-UNC.PROP.MCM-004, REQ-UNC.PROP.MCM-005]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.MODEL.DIST, KRN.MAN.KCI, KRN.NUM.RNG]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# UNC.PROP.MCM.BLOCK: Trial-block sampling and per-trial kernel evaluation

## Purpose

Evaluates one fixed-size block of Monte Carlo trials. Each trial's inputs are drawn from their distributions at a counter-based address, so any block can be computed independently and in any order. Each trial then re-runs every operator of the model through the kernel call interface. The output for a block depends only on (model digest, inputs digest, seed, block index, block size), never on which thread computed it.

## Scope

**In:**
- Sampling address: key = seed, stream = input ordinal in the model's sorted input order (multivariate inputs use consecutive streams per component), index = global trial index
- Per-trial graph evaluation through `evaluate` in topological order
- Failure handling: failed trials recorded by index and excluded
- `BlockOut { block_index, values: per output Vec<f64> in trial order, failed: Vec<u64> }`

**Out:**
- Stopping (owned by `UNC.PROP.MCM.ADAPT`)
- Thread scheduling and the failure-ratio verdict over the whole run (owned by `UNC.PROP.MCM.CMD`)
- Intervals (owned by `UNC.PROP.MCM.INTVL`)

## Requirements

- **REQ-UNC.PROP.MCM.BLOCK-001** For each trial, the function shall sample every input at address (seed, input ordinal, global trial index) and evaluate every operator of the model through `evaluate` on those samples.
- **REQ-UNC.PROP.MCM.BLOCK-002** Given the same model digest, inputs digest, seed, block index and block size, the function shall return byte-identical `BlockOut` on the same platform class, whatever the order in which blocks are computed.
- **REQ-UNC.PROP.MCM.BLOCK-003** If a trial's kernel evaluation fails, then the function shall record the global trial index and the error code, exclude the trial from `values`, and continue with the next trial.
- **REQ-UNC.PROP.MCM.BLOCK-004** If any sampled value or output is NaN or infinite, then the trial shall be recorded as failed with code `non_finite`.

## Acceptance criteria

- **AC-1** Given `fixtures/unc/mcm/block/linear2.json` (Y = X1 + X2, normal inputs) with seed 7, block size 10 000, when blocks 0..3 are computed in order 3, 1, 0, 2 and in order 0..3, then all `BlockOut` bytes match.
- **AC-2** Given the same, then the sample of X1 at trial 12 345 equals `fid-num` normal draw at address (7, 0, 12 345) bit for bit.
- **AC-3** Given `fixtures/unc/mcm/block/fail_fit.json` (a circle fit that fails when a sampled radius is negative), then failed trial indices are listed, values exclude them, and the block completes.
- **AC-4** Given a model whose operator calls are counted by a test kernel table, then each trial calls every operator exactly once.
- **AC-H1** Hypothesis: block-sampled trials reproduce the input-to-output distribution: on the JCGM 101:2008 additive-model example with rectangular inputs (whose output density is known in closed form [unverified: clause 9.2 variant]), the Kolmogorov-Smirnov test of 10^6 trials against the analytic distribution at alpha = 0.01 rejects for a number of 100 independent seeds within the exact two-sided 99 % binomial band for Binomial(100, 0.01), measured by computing blocks in shuffled order; falsified if the rejection count is above the band.

## Design

- **Registry command(s):** none. Algorithm ID `unc.prop.mcm_block@1.0.0`.
- **Data structures:** `BlockSpec { model_digest, inputs_digest, seed, block_index, block_size }`; `BlockOut` canonical encoding (f64 as IEEE-754 hex in JSON or little-endian bytes in the sample format of `UNC.PROP.MCM.SAMP`).
- **Algorithm:** for t in block_index * size .. (block_index + 1) * size: sample, evaluate, store or record failure.
- **Errors:** model and input validation errors before any trial; per-trial errors recorded.
- **Determinism:** D2 (seeded); kernel calls inherit D1 per platform class.
- **Performance budget:** 10 000 trials of a 360-point LS circle fit in under 5 s single-threaded [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::block`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/block/`: hand-written models (CC-BY-4.0); a test kernel table with counting operators.

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::block`, implement `run_block(model, inputs, spec) -> Result<BlockOut, McmError>`. For each global trial index t in the block, sample each input from its `UNC.MODEL.DIST` distribution at counter address (seed, input ordinal in sorted input order, t), with consecutive streams for multivariate components; evaluate every operator of the model graph through the kernel `evaluate` in topological order; store outputs in trial order, or record (t, error code) and exclude the trial on failure or non-finite values. Output must not depend on block computation order. Tests AC-1..AC-4 with `fixtures/unc/mcm/block/`. Constraints: KNP-1 (seeded counter-based RNG, fixed order, no HashMap), single-threaded and I/O-free, CONSTITUTION 8.

## Open questions

- **Applied (failed trials carry their code).** REQ-003 records the error code with the index, so `BlockOut.failed` is a list of `(index, code, detail)` rather than bare indices. Codes: `non_finite` (detail `sample` or `output`), `algorithm_failed` (detail: the algorithm's own code), `sampling_exhausted`, `bad_output` (an operator output that is not one double per output port).
- **Applied (inputs).** `run_block(model, inputs, spec)` takes a `BlockModel` (the validated model bound to the running kernel) and a `SamplingPlan` (the inputs). The inputs digest of `BlockSpec` is SHA-256 of the plan's canonical JSON, which names the model digest, this algorithm ID and each sampler's inputs, kind and stream. If `UNC.PROP.MCM.CMD` later supplies measured-input overrides, they belong in the plan and change its digest.
- **Applied (provenance at L0).** The kernel crate cannot reach the model store (L2). `BlockModel::prepare` accepts only a `ValidModel` (from `UNC.MODEL.GRAPH.CHECK`) and binds it to the running kernel with the same steps as `UNC.MODEL.GRAPH.STORE` AC-6. The release manifest must be the one the model was validated against. The kernel's compiled-in core digest, from the reserved query `kernel.manifest.core_digest@1.0.0`, must equal the manifest's core digest. A kernel without that digest is refused (`provenance{kernel_manifest_unset}`), as the store refuses it. The host wires the kernel's own digest and never derives it from the manifest file. Resolving a `ModelRef` from the store is `UNC.PROP.MCM.CMD`'s step.
- **Applied (edge values).** Every edge carries one scalar double. An operator call gets one input blob per input port, holding the port's values as little-endian doubles in canonical edge order. It must return one little-endian double per output port. Operators get no seed. Kernel errors that do not depend on the trial (floating-point environment, lookup, card, arity, digest, parameter validation) end the block with `kernel{code, trial}` instead of being counted as trial failures.
- **Applied (correlation groups).** A model correlation group is drawn as one `UNC.MODEL.DIST` multivariate normal at the stream of its first (smallest-ID) input. Its components use DIST's multivariate sub-stream, addressed by component, rather than separate consecutive user streams. The other members' ordinals stay unused, so independent inputs keep their own ordinal streams. A group with a non-normal member is refused (`unsupported_model{correlated_non_normal}`) because no copula is specified. `UNC.MODEL.DIST` or `UNC.MODEL.GRAPH` would have to specify one first.
- **Applied (AC-2 wording).** `fid-num` has no normal draw of its own. "The `fid-num` normal draw at (7, 0, 12 345)" is read as `UNC.MODEL.DIST`'s normal sampler on the `fid-num` Philox block at that address.
- **Applied (AC-H1 test).** The Kolmogorov-Smirnov critical value is the asymptotic 0.99 quantile of the Kolmogorov distribution (1.62762, from the oracle), with Stephens' finite-sample factor sqrt(n) + 0.12 + 0.11 / sqrt(n) [unverified: Stephens 1970 modification]. The band is the exact two-sided 99 % band of Binomial(100, 0.01), [0, 4]. CI runs 100 seeds at 2000 trials (4 shuffled blocks of 500) on every target; the hypothesis scale runs with `UNC_MCM_BLOCK_FULL=1`.
- **Measured (#338; `UNC_MCM_BLOCK_FULL=1`, release, x86-64 Windows).** AC-H1 at 10^6 trials per seed (100 shuffled blocks of 10 000): the KS test rejected 1 of 100 seeds, inside the band [0, 4]; the largest scaled statistic was 1.6313 against 1.6276. The CI smoke (2000 trials per seed) also rejects 1 of 100. Supported.
- **Open (performance).** The budget of 10 000 trials of a 360-point LS circle fit in under 5 s is not measured: no LS circle-fit kernel algorithm is registered yet. It should be measured in `UNC.PROP.MCM.CMD` once one is.

## Children

_Leaf: none._
