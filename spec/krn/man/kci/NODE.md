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
built_in: [fid-kci]
built_pr: 59
satisfies: [REQ-KRN.MAN-001, REQ-KRN.MAN-002]
covers: []
depends_on: [KRN.NUM.ERR, KRN.MAN.CARD]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- **REQ-KRN.MAN.KCI-008** If a call's number of inputs is outside the `input_arity` declared by the algorithm's spec card (`KRN.MAN.CARD` REQ-006), then `evaluate` shall return `KernelError::InputArityMismatch{expected, got}` before hashing any input or calling any algorithm code.
- **REQ-KRN.MAN.KCI-009** If an algorithm's own declared input arity (`KernelAlgorithm::input_arity`) differs from its spec card's `input_arity`, then `AlgorithmTable::new` shall return `KernelError::CardArityDrift{id, card, implementation}` and build no table.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/sha256-cavp-v1` (NIST CAVP SHA-256 byte-oriented test vectors), when hashed, then every digest matches.
- **AC-2** Given a test algorithm `test.echo.identity@1.0.0` registered in a test table with a spec card, and fixture `fixtures/man/kci-calls-v1` (20 calls), when `evaluate` runs on x86-64, ARM64 and wasm32 under wasmtime, then every `KernelResult` is byte-identical across targets and equals the golden bytes.
- **AC-3** (error path) Given a call whose second input's claimed digest has one flipped bit, then `evaluate` returns `DigestMismatch{input_index: 1}` and the test algorithm's call counter stays 0.
- **AC-4** (error path) Given an unknown algorithm ID `fit.nothing.ls@1.0.0`, then `evaluate` returns `UnknownAlgorithm`; given a registered algorithm without a card, `MissingSpecCard`.
- **AC-5** Given the crate source, when the KNP-1 lint runs, then it reports no finding (no I/O, no clock, no environment, no `unsafe`).
- **AC-6** (error path) Given a test algorithm whose card declares `input_arity = "2"`, when `evaluate` runs with 0, 1, 3, 4 or 5 inputs (and with 3 inputs one of which has a wrong digest), then it returns `InputArityMismatch` and the algorithm's validate and run counter stays 0; with 2 inputs it runs. Through the WASM module, `fixtures/man/wasm/calls-v1.txt` row `recurse-input-arity` gives `{"error":"input_arity_mismatch"}`.
- **AC-7** (error path) Given a card and an algorithm whose input arities differ, `AlgorithmTable::new` returns `CardArityDrift`; for every algorithm registered today (the stand-ins of `fid-kernel-wasm`, the test algorithms here) the card arity equals the implementation's, and the stand-ins' `run`, called directly, accepts exactly the input counts its card admits.

## Design

- **Registry command(s):** none; the command bus (`CORE.BUS`) calls `evaluate` through its host adapter.
- **Data structures:** `KernelCall { algorithm: AlgorithmId, params: Vec<u8>, seed: Option<u64>, inputs: Vec<(Vec<u8>, [u8; 32])> }`; `KernelResult { algorithm, params_digest, input_digests, seed, output: Vec<u8>, output_digest }`; `trait KernelAlgorithm { fn id(&self) -> AlgorithmId; fn input_arity(&self) -> InputArity; fn validate(&self, params: &[u8]) -> Result<(), KernelError>; fn run(&self, params: &[u8], seed: Option<u64>, inputs: &[&[u8]]) -> Result<Vec<u8>, KernelError>; }`; `AlgorithmTable` as a sorted `Vec` searched by binary search (no hash maps).
- **Algorithm:** SHA-256 per FIPS 180-4 (portable u32 code, no intrinsics); canonical JSON: object keys sorted by UTF-16 code units, no insignificant whitespace, strings escaped per RFC 8785, numbers never emitted (all numerics are strings by kernel convention).
- **Errors:** `KernelError::{DigestMismatch, UnknownAlgorithm, MissingSpecCard, InvalidParams, FpEnvironment, Algorithm(String-free typed inner), InputArityMismatch, CardArityDrift}` (and `DuplicateAlgorithm` from table construction).
- **Check order in `evaluate`:** FP probe, algorithm lookup, spec card, input count against the card's `input_arity`, input digests, parameter validation, run. Every check before `run` fails closed and runs no algorithm code; the arity check comes before hashing so an oversized call is refused cheaply.
- **Determinism:** D0.
- **Performance budget:** SHA-256 at >= 200 MB/s on reference hardware [unverified until measured].
- **Crate:** `fid-kci`.

## Test fixtures

`fixtures/man/sha256-cavp-v1`: NIST CAVP SHA-256 vectors (US government work, public domain [unverified]). `fixtures/man/kci-calls-v1`: synthetic calls with golden results, CC-BY-4.0.

## Build prompt

> Create crate `fid-kci` (pure Rust, no I/O, no `unsafe`, no clock or environment access; depends only on `fid-num`). Implement portable SHA-256 (FIPS 180-4; no intrinsics, no runtime dispatch), a canonical JSON writer following RFC 8785 for strings, arrays and objects (keys sorted by UTF-16 code units; numbers are always emitted as strings by kernel convention), `AlgorithmId` (`<module>.<feature>.<method>@<major>.<minor>.<patch>`), `KernelCall`, `KernelResult`, `KernelError`, `trait KernelAlgorithm` and `AlgorithmTable` (sorted Vec, binary search). Implement `pub fn evaluate(table: &AlgorithmTable, cards: &CardRegistry, call: &KernelCall) -> Result<KernelResult, KernelError>`: run `fid_num::err::probe_fp_env`; re-hash every input and return `DigestMismatch{input_index}` before any algorithm runs; reject unknown IDs, IDs without spec card and invalid params; run; hash params and output; return the envelope. Tests: NIST CAVP SHA-256 vectors `fixtures/man/sha256-cavp-v1`; `fixtures/man/kci-calls-v1` golden results byte-identical on x86-64/ARM64/wasm32 (wasmtime); digest mismatch with zero algorithm calls; unknown ID and missing card; KNP-1 lint clean. Deny `unwrap`/`expect`/`panic`. Acceptance: AC-1..AC-5 of spec node KRN.MAN.KCI.

## Open questions

- **SHA-256 test vectors (AC-1, REQ-KRN.MAN.KCI-007):** `fixtures/man/sha256-cavp-v1` currently holds CAVP-layout vectors with pseudo-random messages whose digests come from Python `hashlib`, plus the FIPS 180-4 example messages and a few NIST CAVP sample vectors checked against `hashlib`. The real NIST `SHA256ShortMsg.rsp` and `SHA256LongMsg.rsp` files are pending the D-038 third-party data decision; the test reader (`check_rsp` in `crates/fid-kci/tests/kci.rs`) already parses that format, so adding them needs no code change.

- The `KernelResult` here carries only kernel-owned digest fields; the full ResultRecord envelope (uncertainty, decision, spec context) is assembled by `CORE`, `UNC` and `GDT` around it.

## Resolved at build

- **Input arity (2026-11-20, REQ-008, REQ-009).** Before this change `evaluate` passed any number of inputs to `run`, so each algorithm had to refuse a wrong count itself (raised by `UNC.MODEL.GRAPH.CHECK`). Neither the card nor the manifest declared an algorithm's inputs, so `KRN.MAN.CARD` gained the required `input_arity` field (REQ-KRN.MAN.CARD-006) and `KernelAlgorithm` gained `input_arity()`. The card is the published declaration and `evaluate` checks against it; the trait method lets `AlgorithmTable::new` refuse a card that drifted from its implementation. Only the count is declared: kernel inputs are opaque byte blobs, so input kinds, units and names stay with the port-signature follow-up recorded in `UNC.MODEL.GRAPH.CHECK`. Like the other kernel checks, an algorithm returning `InputArityMismatch` or `CardArityDrift` itself is confined to `Algorithm(IMPERSONATION_CODE)`.

## Children

_Leaf: none._
