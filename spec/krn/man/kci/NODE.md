---
id: KRN.MAN.KCI
title: Kernel call interface and digests
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN-001, REQ-KRN.MAN-002]
covers: []
depends_on: [KRN.NUM.ERR, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# KRN.MAN.KCI: Kernel call interface and digests

## Purpose

The kernel's single, pure entry point, `evaluate(call) -> result`. A call names an algorithm ID, typed parameters, an optional seed and input byte blobs with claimed digests. The kernel re-hashes every input itself, refuses mismatches, dispatches to the registered algorithm and hashes its own output, so every digest in a result is produced inside the identified module. It also provides the kernel's SHA-256 and the canonical JSON encoder used for all kernel digests.

## Scope

**In:** `KernelCall`, `KernelResult` envelope (kernel-owned fields), `KernelError`; the `KernelAlgorithm` trait and an `AlgorithmTable` that the assembly crate fills; input re-hashing and `digest_mismatch`; output digest; an in-house SHA-256; a canonical JSON writer (RFC 8785 rules restricted to strings, integers-as-strings, arrays and objects); the FP-environment probe at entry.
**Out:** algorithms (`KRN.LSQ`, `KRN.MZ`, `KRN.ENV`, `KRN.CONS`, `KRN.OUT`); spec cards (`KRN.MAN.CARD`); the manifest (`KRN.MAN.MANI`); the host adapter, blob store and journal (`CORE`); formatters (`KRN.NUM.FMT`).

## Requirements

- **REQ-KRN.MAN.KCI-001** The kernel shall expose exactly one public evaluation function, `evaluate(&AlgorithmTable, &KernelCall) -> Result<KernelResult, KernelError>`, with no global or thread-local state.
- **REQ-KRN.MAN.KCI-002** `KernelCall` shall contain the algorithm ID with exact semver, the canonical parameter bytes, an optional seed (u64) and a list of input blobs, each with its claimed SHA-256 digest.
- **REQ-KRN.MAN.KCI-003** If any input's computed SHA-256 differs from its claimed digest, then `evaluate` shall return `KernelError::DigestMismatch{input_index}` before calling any algorithm.
- **REQ-KRN.MAN.KCI-004** If the algorithm ID is not in the table, has no spec card, or the parameters fail the algorithm's validation, then `evaluate` shall return a typed error and compute nothing.
- **REQ-KRN.MAN.KCI-005** On success, `KernelResult` shall carry the algorithm ID, parameter digest, input digests, seed, output canonical bytes and output digest, all computed inside `evaluate`.
- **REQ-KRN.MAN.KCI-006** `evaluate` shall run the FP-environment probe of `KRN.NUM.ERR` first and return its error if the environment is not IEEE round-to-nearest-even without flush-to-zero.
- **REQ-KRN.MAN.KCI-007** The SHA-256 implementation shall pass the NIST FIPS 180-4 example vectors and the CAVP short and long message tests, and the canonical JSON writer shall produce byte-identical output on every target.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/sha256-cavp-v1` (NIST CAVP SHA-256 byte-oriented test vectors), when hashed, then every digest matches.
- **AC-2** Given a test algorithm `test.echo.identity@1.0.0` registered in a test table with a spec card, and fixture `fixtures/man/kci-calls-v1` (20 calls), when `evaluate` runs on x86-64, ARM64 and wasm32 under wasmtime, then every `KernelResult` is byte-identical across targets and equals the golden bytes.
- **AC-3** (error path) Given a call whose second input's claimed digest has one flipped bit, then `evaluate` returns `DigestMismatch{input_index: 1}` and the test algorithm's call counter stays 0.
- **AC-4** (error path) Given an unknown algorithm ID `fit.nothing.ls@1.0.0`, then `evaluate` returns `UnknownAlgorithm`; given a registered algorithm without a card, `MissingSpecCard`.
- **AC-5** Given the crate source, when the KNP-1 lint runs, then it reports no finding (no I/O, no clock, no environment, no `unsafe`).

## Design

- **Registry command(s):** none; the command bus (`CORE.BUS`) calls `evaluate` through its host adapter.
- **Data structures:** `KernelCall { algorithm: AlgorithmId, params: Vec<u8>, seed: Option<u64>, inputs: Vec<(Vec<u8>, [u8; 32])> }`; `KernelResult { algorithm, params_digest, input_digests, seed, output: Vec<u8>, output_digest }`; `trait KernelAlgorithm { fn id(&self) -> AlgorithmId; fn validate(&self, params: &[u8]) -> Result<(), KernelError>; fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError>; }`; `AlgorithmTable` as a sorted `Vec` searched by binary search (no hash maps).
- **Algorithm:** SHA-256 per FIPS 180-4 (portable u32 code, no intrinsics); canonical JSON: object keys sorted by UTF-16 code units, no insignificant whitespace, strings escaped per RFC 8785, numbers never emitted (all numerics are strings by kernel convention).
- **Errors:** `KernelError::{DigestMismatch, UnknownAlgorithm, MissingSpecCard, InvalidParams, FpEnvironment, Algorithm(String-free typed inner)}`.
- **Determinism:** D0.
- **Performance budget:** SHA-256 at >= 200 MB/s on reference hardware [unverified until measured].
- **Crate:** `fid-kci`.

## Test fixtures

`fixtures/man/sha256-cavp-v1`: NIST CAVP SHA-256 vectors (US government work, public domain [unverified]). `fixtures/man/kci-calls-v1`: synthetic calls with golden results, CC-BY-4.0.

## Build prompt

> Create crate `fid-kci` (pure Rust, no I/O, no `unsafe`, no clock or environment access; depends only on `fid-num`). Implement portable SHA-256 (FIPS 180-4; no intrinsics, no runtime dispatch), a canonical JSON writer following RFC 8785 for strings, arrays and objects (keys sorted by UTF-16 code units; numbers are always emitted as strings by kernel convention), `AlgorithmId` (`<module>.<feature>.<method>@<major>.<minor>.<patch>`), `KernelCall`, `KernelResult`, `KernelError`, `trait KernelAlgorithm` and `AlgorithmTable` (sorted Vec, binary search). Implement `pub fn evaluate(table: &AlgorithmTable, cards: &CardRegistry, call: &KernelCall) -> Result<KernelResult, KernelError>`: run `fid_num::err::probe_fp_env`; re-hash every input and return `DigestMismatch{input_index}` before any algorithm runs; reject unknown IDs, IDs without spec card and invalid params; run; hash params and output; return the envelope. Tests: NIST CAVP SHA-256 vectors `fixtures/man/sha256-cavp-v1`; `fixtures/man/kci-calls-v1` golden results byte-identical on x86-64/ARM64/wasm32 (wasmtime); digest mismatch with zero algorithm calls; unknown ID and missing card; KNP-1 lint clean. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.MAN.KCI.

## Open questions

- The `KernelResult` here carries only kernel-owned digest fields; the full ResultRecord envelope (uncertainty, decision, spec context) is assembled by `CORE`, `UNC` and `GDT` around it.

## Children

_Leaf: none._
