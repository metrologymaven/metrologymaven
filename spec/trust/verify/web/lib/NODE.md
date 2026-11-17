---
id: TRUST.VERIFY.WEB.LIB
title: wasm32 verifier library build and CLI parity test
level: 4
parent: TRUST.VERIFY.WEB
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY.WEB-001]
covers: []
depends_on: [TRUST.VERIFY.REPORT]
uses_artifacts: [TRUST.PKG.FIXTURE, TRUST.VERIFY.CLI]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.VERIFY.WEB.LIB: wasm32 verifier library build and CLI parity test

## Purpose

Builds the verifier library (package reader, signature core, chain checks, replay pipeline and report composer) for `wasm32-unknown-unknown` behind one entry point, `verify(source, options) -> ReportJson`, and proves in CI that its JSON report for every golden package is byte-identical to `fid-verify --json` on the same inputs.

## Scope

**In:**
- `fid-verify-web` crate: wasm32 build of the library entry point
- Feature flags that drop native-only code (filesystem, process exit) from the wasm build
- Parity test runner (headless wasm runtime in CI) over the golden set

**Out:**
- Browser file streaming (`TRUST.VERIFY.WEB.STREAM`)
- Replay in a worker (`TRUST.VERIFY.WEB.WREPLAY`)
- The page (`TRUST.VERIFY.WEB.SHELL`)

## Requirements

- **REQ-TRUST.VERIFY.WEB.LIB-001** The library shall build for `wasm32-unknown-unknown` with no filesystem, network, clock or threading imports beyond those `wasm-bindgen` generates, checked by an import-section scan in CI.
- **REQ-TRUST.VERIFY.WEB.LIB-002** For every golden package and trust-root fixture, the wasm build's JSON report shall be byte-identical to `fid-verify --json` with the same inputs and options, except replay, which `TRUST.VERIFY.WEB.WREPLAY` covers.
- **REQ-TRUST.VERIFY.WEB.LIB-003** The entry point shall take a byte-source trait object, an optional trust-root file, an optional revocation file and the report options, and shall return the report JSON or a typed error with the same codes as the CLI.
- **REQ-TRUST.VERIFY.WEB.LIB-004** Where the CLI report includes host-dependent fields (verifier build ID, platform), the wasm build shall fill them with its own values and the parity test shall compare all other fields byte for byte.

## Acceptance criteria

- **AC-1** Given the golden set `fixtures/pkg/golden/` and `fixtures/trust-root/`, when `ci/verify-web/parity.sh` runs the wasm library in a headless runtime, then every report equals the CLI report outside the host fields.
- **AC-2** (error path) Given the malformed corpus `fixtures/pkg/malformed/`, then the wasm library returns the same error codes as the CLI and never traps.
- **AC-3** Given the import scan, then the module's imports are only `wasm-bindgen` glue.

## Design

- **Registry command(s):** none (not a registry surface; the page is a separate distribution of `verify.package@1` logic).
- **Data structures:** `trait ByteSource { fn len(&self) -> u64; fn read_at(&self, off: u64, buf: &mut [u8]) -> Result<usize, SrcErr>; }` (async variant for the browser).
- **Algorithm:** same composition as `fid-verify`'s library (report module); host fields from a build constant.
- **Errors:** CLI error codes.
- **Determinism:** D0.
- **Performance budget:** chain and signature check of a 100 MB golden package < 10 s in a headless browser [unverified until measured].
- **Crate:** `fid-verify-web` (lib, `cdylib`), depending only on the `fid-verify` library allowlist crates.

## Test fixtures

Golden and malformed packages from `TRUST.PKG.FIXTURE`; expected CLI reports produced by `TRUST.VERIFY.CLI` in CI.

## Build prompt

> Create crate `fid-verify-web` (`cdylib`, target `wasm32-unknown-unknown`, `wasm-bindgen`) exposing `verify(source: ByteSource, trust_root: Option<Bytes>, revoked: Option<Bytes>, opts: Options) -> Result<String /* report JSON */, VerifyError>`, composed from the same library crates as `fid-verify` (`fid-pkg` reader without feature `export`, `fid-sign-core`, `fid-chain`, `fid-vreplay`, report module). Gate out filesystem and process code by features. Add `ci/verify-web/parity.sh`: run the wasm build headless over `fixtures/pkg/golden/` and compare each report with `fid-verify --json` (ignore only host fields: build ID, platform); and an import-section scan allowing only wasm-bindgen glue. Tests: AC-1..AC-3. Constraints: REQ-TRUST.VERIFY-002/009, CONSTITUTION §8.

## Open questions

- None.

## Children

_Leaf: none._
