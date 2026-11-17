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
satisfies: [REQ-UNC.PROP.MCM-004]
covers: []
depends_on: [UNC.PROP.MCM.BLOCK, KRN.MAN.KCI]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-17
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

## Children

_Leaf: none._
