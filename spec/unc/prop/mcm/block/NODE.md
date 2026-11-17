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
satisfies: [REQ-UNC.PROP.MCM-001, REQ-UNC.PROP.MCM-004, REQ-UNC.PROP.MCM-005]
covers: []
depends_on: [UNC.MODEL.GRAPH, UNC.MODEL.DIST, KRN.MAN.KCI, KRN.NUM.RNG]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

- None.

## Children

_Leaf: none._
