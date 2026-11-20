---
id: UNC.PROP.MCM.SAMP
title: Retained sample sets and their digest
level: 4
parent: UNC.PROP.MCM
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-kunc]
built_pr: 340
satisfies: [REQ-UNC.PROP.MCM-004]
covers: []
depends_on: [UNC.PROP.MCM.BLOCK, KRN.MAN.KCI]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-20
---

# UNC.PROP.MCM.SAMP: Retained sample sets and their digest

## Purpose

Defines the canonical byte format `mcm-samples@1` in which Monte Carlo output samples are retained for `UNC.DEC` (conformance probability) and `UNC.XFM` (fields), and its SHA-256 digest, so that a sample set is addressed by digest and two runs with the same inputs produce the same digest. Storage of the bytes is done by the command layer, not here.

## Scope

**In:**
- Header: format version, model digest, inputs digest, seed, profile digest, output names, trial count, failed-trial count
- Body: per output, little-endian f64 in global trial order with failed trials omitted; failed-trial index list
- Size cap (default 64 MiB [unverified]); over the cap no bytes are produced and the result carries `samples_not_retained`
- Encoder, bounded decoder and digest

**Out:**
- Storing bytes in the blob store (owned by `UNC.PROP.MCM.CMD`)
- Joint sample use for vectors (owned by `UNC.PROP.MVAR`)

## Requirements

- **REQ-UNC.PROP.MCM.SAMP-001** The encoder shall produce bytes determined only by the header fields and the block outputs in block-index order, and the digest shall be SHA-256 over those bytes.
- **REQ-UNC.PROP.MCM.SAMP-002** If the encoded size would exceed the cap, then the encoder shall produce no bytes and return `samples_not_retained` with the size it would have had.
- **REQ-UNC.PROP.MCM.SAMP-003** The decoder shall reject input whose declared counts disagree with its length, or whose version is unknown, with `samples_invalid`.

## Acceptance criteria

- **AC-1** Given `BlockOut`s from `UNC.PROP.MCM.BLOCK` AC-1, when encoded from blocks computed in two different orders, then the bytes and digest are identical and equal `fixtures/unc/mcm/samp/linear2.sha256`.
- **AC-2** Given a cap of 1 KiB and 10 000 trials, then `samples_not_retained` with the would-be size.
- **AC-3** (error path) Given an encoding truncated by 8 bytes, then decoding fails `samples_invalid`.
- **AC-4** Given AC-1 encoded and decoded, then the round trip is lossless.
- **AC-H1** Hypothesis: the default retention cap keeps the complete `mcm-samples@1` set for at least 95 % [unverified] of runs on the JCGM 101:2008 worked examples (additive model, mass calibration, comparison loss, gauge-block calibration [unverified: clause 9 numbering]), encoded from their published input definitions with the published results as reference values and the ORS-UNC known-truth cases `nonlin-rect`, `mvar-datum` of `UNC.ORS.UNC.BASE` (the `fid-ors` release named in the run record) at the trial counts `UNC.PROP.MCM.ADAPT` reaches, and a conformance probability recomputed from the decoded set equals the in-memory value bit for bit, measured by recording `samples_not_retained` outcomes and comparing probabilities; falsified if retention falls below 95 % or any recomputed value differs.

## Design

- **Registry command(s):** none.
- **Data structures:** `SampleSetHeader`; `SampleSet { header, outputs: Vec<Vec<f64>>, failed: Vec<u64> }`; header in RFC 8785 JSON with a length prefix, then the binary body.
- **Algorithm:** concatenate blocks by block index; write; hash with the kernel SHA-256.
- **Errors:** `samples_not_retained{size}`, `samples_invalid`.
- **Determinism:** D2 (content is seeded); encoding itself is D0.
- **Performance budget:** encode 10^6 samples in under 50 ms [unverified until measured].
- **Crate:** `fid-kunc` (module `prop::mcm::samples`; KNP-1 applies).

## Test fixtures

`fixtures/unc/mcm/samp/`: expected digest for the BLOCK linear fixture (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `prop::mcm::samples`, define the `mcm-samples@1` format: a length-prefixed RFC 8785 JSON header (version, model digest, inputs digest, seed, profile digest, output names, trial and failure counts) followed by per-output little-endian f64 values in global trial order (failed trials omitted) and the failed-index list. Provide `encode(blocks, header, cap)`, which returns bytes and a SHA-256 digest or `samples_not_retained{size}` over the cap (default 64 MiB), and a bounded `decode` that rejects inconsistent lengths or unknown versions (`samples_invalid`). Tests AC-1..AC-4. Constraints: KNP-1, I/O-free, CONSTITUTION 8.

## Open questions

- The 64 MiB default cap is unmeasured against DEC and XFM needs [unverified].
- **Applied (byte layout).** `u64` little-endian header length `H`; `H` bytes of RFC 8785 header `{"failed","inputs","model","outputs":[...],"profile","seed","trials","version":"mcm-samples@1"}` (integers as decimal strings, outputs in ascending byte order, the column order of `BlockOut`); per output `trials - failed` little-endian f64; then `failed` little-endian `u64` indices, strictly ascending. Size `8 + H + 8 (K (T - F) + F)`. The full rules are in the module documentation of `fid-kunc` `prop::mcm::samples`.
- **Applied (failure codes not retained).** BLOCK records `(index, code, detail)` per failed trial. Scope names only the failed-index list, so the codes are not in the set; they stay in the blocks and belong in the run record (`UNC.PROP.MCM.CMD`).
- **Applied (`encode(blocks, header, cap)`).** The header carries `trials` and `failed`, and the encoder checks them against the blocks. The blocks must be exactly blocks `0 .. n` of one size, with one column per header output, finite values and failed indices inside each block's range. Anything else is `samples_invalid{detail}`. The checks run before the cap test. Errors are `McmError::SamplesNotRetained{size}` and `McmError::SamplesInvalid{detail}` (details: `blocks`, `columns`, `block_size`, `non_finite`, `failed`, `trials`, `failed_count`, `outputs`, `counts`; for the decoder also `too_large`, `length`, `header`, `version`).
- **Applied (bounded decoder).** `decode(bytes, cap)` refuses input over `cap` (`too_large`) and headers over 1 MiB. It accepts only the canonical header bytes, so one sample set has exactly one encoding and one digest. REQ-003's count and length mismatch is `samples_invalid{length}`, and an unknown version is `samples_invalid{version}`.
- **Applied (oracle bit exactness).** The digest pins every sample bit. A correctly rounded 60-digit ln differs from the crate's fdlibm ln in the last place for about 2.6 % of AS241 tail calls, and that changes one linear2 sample. So `fixtures/unc/mcm/samp/make_fixtures.py` ports the published fdlibm logarithm to Python binary64 (no platform libm) and checks every call against the 60-digit ln (within 1 ulp). Its linear2 and fail_fit digests match the crate on every CI target, wasm32 included.
- **Open (profile digest; for UNC.PROP.MCM.ADAPT or UNC.PROP.MCM.CMD).** The header's `profile` digest is opaque here. `AdaptProfile` has no canonical form or digest yet. ADAPT or CMD should define one so that the header names the profile the run used.
- **Open (storage; for UNC.PROP.MCM.CMD).** CMD stores the bytes under their digest and records `samples_not_retained{size}` in the run record when the cap is exceeded.
- **Measured (performance; release, x86-64 Windows, 3 runs).** Encoding 10^6 samples took 51 to 57 ms with the digest included. SHA-256 alone took 43 to 48 ms (the kernel's portable SHA-256, `KRN.MAN.KCI`), and the byte layout took about 8 ms. The 50 ms budget is met by the layout but not with the digest. A faster digest is a `KRN.MAN.KCI` matter.
- **Deferred (AC-H1).** AC-H1 needs `UNC.DEC` (conformance probability), `UNC.ORS.UNC.BASE` (known-truth cases) and `UNC.PROP.MCM.CMD` (runs at ADAPT's trial counts), none of them built. When it is measured, the retention claim should use an exact one-sided binomial band, since only low retention falsifies: with N runs, falsified if the retained count is below the largest k with P(K < k) <= 0.05 under Binomial(N, 0.95). For the 6 runs named (4 JCGM 101:2008 clause 9 examples [unverified: clause 9 numbering] and 2 ORS cases), that is k = 5 (P(K < 5) = 0.0328). The bit-for-bit recomputed probability has no band.

## Children

_Leaf: none._
