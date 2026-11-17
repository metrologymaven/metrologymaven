---
id: TRUST.PKG.FIXTURE
title: Published package spec and conformance fixtures
level: 3
parent: TRUST.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG-001, REQ-TRUST.PKG-007, REQ-TRUST.PKG-008]
covers: []
depends_on: [TRUST.PKG.MANIFEST, TRUST.SIGN.DSSE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.PKG.FIXTURE: Published package spec and conformance fixtures

## Purpose

The public face of the format: a human-readable Result Package specification (CC-BY-4.0), the JSON Schemas, a set of golden packages with pinned digests and a malformed-package corpus with expected error codes. Third-party verifiers use these to claim conformance, and every TRUST crate tests against them.

## Scope

**In:**
- `docs/result-package/v1.md` specification text written by the project (no standards text)
- JSON Schemas for manifest and predicate
- Golden packages covering each evidence tier, with pinned SHA-256
- Malformed corpus: one file per failure class, each with `expect.json`
- A deterministic generator that rebuilds every fixture from seeds
- A conformance runner script interface (`fixtures/pkg/run-conformance`) that any implementation can call

**Out:**
- The format code (`TRUST.PKG.MANIFEST`, `TRUST.PKG.READER`)
- Replay conformance packages with real kernel outputs (`TRUST.REPLAY.CORPUS`)
- Hosting the spec on the website (`OUTR`)

## Requirements

- **REQ-TRUST.PKG.FIXTURE-001** The specification shall be published under CC-BY-4.0, state its format version, and describe every member, field and error code defined by `TRUST.PKG.MANIFEST` and `TRUST.PKG.READER`.
- **REQ-TRUST.PKG.FIXTURE-002** The golden set shall include at least one package per evidence tier (`results`, `features`, `points`), one with two signatures and one with a UEC-0 result.
- **REQ-TRUST.PKG.FIXTURE-003** The malformed corpus shall include at least one case per reader error code and per limit, and each case shall name its expected error code in `expect.json`.
- **REQ-TRUST.PKG.FIXTURE-004** Running the fixture generator twice shall reproduce every fixture byte for byte, and CI shall fail if a committed fixture differs from its regenerated form.
- **REQ-TRUST.PKG.FIXTURE-005** A CI job shall fail when a spec field or error code exists in code but not in the specification, or the reverse.

## Acceptance criteria

- **AC-1** Given a clean checkout, when `fixtures/pkg/generate --check` runs, then it exits 0 and reports every golden and malformed fixture as reproduced.
- **AC-2** Given `fixtures/pkg/malformed/`, when the reference reader runs over it via `fixtures/pkg/run-conformance`, then every case yields its `expect.json` code.
- **AC-3** (error path) Given a deliberately edited golden fixture, when `generate --check` runs, then it exits non-zero naming the fixture.
- **AC-4** Given the spec drift check, when an error code is added to the reader without a spec entry, then CI fails naming that code.

## Design

- **Registry command(s):** none.
- **Data structures:** `expect.json { error_code, note }`; generator seed file `fixtures/pkg/seeds.toml`.
- **Algorithm:** generator uses `fid-pkg::manifest` to build goldens; malformed cases are produced by explicit byte mutations of goldens (documented per case); drift check extracts error-code enums from the crate and `###` code headings from the spec.
- **Errors:** generator reports `fixture.mismatch`.
- **Determinism:** D0.
- **Performance budget:** full regeneration in under 60 s in CI [unverified until measured].
- **Crate:** `fid-pkg` (dev-only `xtask` binary `pkg-fixtures`).

## Test fixtures

All fixtures are synthetic and generated (CC-BY-4.0); no third-party data.

## Build prompt

> In the `fid-pkg` crate's xtask, build `pkg-fixtures`, a deterministic generator for the Result Package v1 conformance set, and write `docs/result-package/v1.md` (original text, CC-BY-4.0) plus JSON Schemas for the manifest and predicate. Golden packages: one per evidence tier (`results`, `features`, `points`), one with two DSSE signatures (test keys, clearly labelled as test-only), one with a UEC-0 result; pin each SHA-256. Malformed corpus: one case per reader error code and per limit, each made by a documented byte mutation of a golden, each with `expect.json` naming the expected code. Provide `generate --check` (fail if regeneration differs) and a `run-conformance` interface that runs any reader binary over the corpus. Add a CI drift check between the crate's error-code enum and the spec. Tests: the four acceptance criteria. Constraints: never paste standards text; D0; no panics in the generator.

## Open questions

- Test signing keys for the two-signature golden must be generated deterministically and published; confirm with `TRUST.SIGN.DSSE` that deterministic Ed25519 test keys are acceptable.

## Children

_Leaf: none._
