---
id: TRUST.SIGN.INTEROP
title: Third-party DSSE interop test and envelope fuzz corpus
level: 3
parent: TRUST.SIGN
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN-002, REQ-TRUST.SIGN-008]
covers: []
depends_on: [TRUST.SIGN.DSSE, TRUST.PKG.FIXTURE]
side_effect: R1
determinism: D2
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.SIGN.INTEROP: Third-party DSSE interop test and envelope fuzz corpus

## Purpose

Proves REQ-TRUST-001's promise that off-the-shelf tooling can verify our envelopes, and that tampering is always caught. It adds a CI job that verifies golden envelopes with a pinned third-party in-toto/DSSE implementation, and a seeded mutation corpus of at least 10,000 envelopes run through `TRUST.SIGN.DSSE`.

## Scope

**In:**
- CI job running a pinned third-party verifier on golden envelopes (both directions: they verify ours; we verify theirs)
- Seeded mutation generator and the corpus run
- A `cargo fuzz` target on envelope parsing and verification

**Out:**
- The DSSE implementation (`TRUST.SIGN.DSSE`)
- Golden package contents (`TRUST.PKG.FIXTURE`)

## Requirements

- **REQ-TRUST.SIGN.INTEROP-001** A CI job shall verify the Ed25519 and P-256 golden envelopes with the in-toto reference implementation at a pinned version, and shall fail if either does not verify [unverified: that its DSSE verifier accepts both algorithms].
- **REQ-TRUST.SIGN.INTEROP-002** The same job shall create envelopes with the third-party tool for the same statement and verify them with `TRUST.SIGN.DSSE`.
- **REQ-TRUST.SIGN.INTEROP-003** The mutation generator shall produce at least 10,000 distinct mutated envelopes from a fixed seed, covering payload, payloadType, keyid, signature, base64 and JSON-structure mutations.
- **REQ-TRUST.SIGN.INTEROP-004** Running the corpus shall yield zero accepted mutants and zero panics, and CI shall fail otherwise.

## Acceptance criteria

- **AC-1** Given `fixtures/dsse/golden/*.dsse.json` and the pinned tool in `ci/interop/requirements.lock`, when `ci/interop/run.sh` runs, then all golden envelopes verify with the third-party tool and the tool's envelopes verify with ours.
- **AC-2** Given seed `0x5EED_0001`, when `cargo xtask dsse-mutants --count 10000` runs twice, then both corpora are identical and none is accepted by the verifier.
- **AC-3** (error path) Given a deliberately weakened verifier (test feature that skips the payloadType in PAE), when the corpus runs, then at least one mutant is accepted and the job fails, showing the corpus detects the weakness.
- **AC-4** (fuzz) Given the fuzz target run for 10 minutes in CI, then no crash or hang occurs.

## Design

- **Registry command(s):** none.
- **Data structures:** mutant record `{ id, mutation_kind, base_golden }`.
- **Algorithm:** counter-based seeded RNG picks golden, mutation kind and offset; mutants deduplicated by SHA-256; corpus regenerated in CI rather than committed.
- **Errors:** job fails with the mutant ID on any false accept.
- **Determinism:** D2 (seeded).
- **Performance budget:** corpus run under 60 s in CI [unverified until measured].
- **Crate:** `fid-sign-core` (dev xtask `dsse-mutants`; CI scripts under `ci/interop/`).

## Test fixtures

- Golden envelopes and test keys from `TRUST.SIGN.DSSE` / `TRUST.PKG.FIXTURE`.
- Third-party tool pinned by version and hash; licence checked against the SPDX allowlist for CI-only tools.

## Build prompt

> Add CI interop and tamper testing for `fid-sign-core`. (1) `ci/interop/run.sh`: install the in-toto reference implementation at a pinned version (lock file with hashes), verify every `fixtures/dsse/golden/*.dsse.json` with it, then have it sign `fixtures/dsse/statement.json` with the published test keys and verify those envelopes with `fid-sign-core`. If the reference implementation cannot handle one algorithm, document it and pick a second pinned tool for that algorithm. (2) `cargo xtask dsse-mutants --count N --seed S`: deterministic mutants (payload, payloadType, keyid, signature, base64 and JSON-structure mutations), deduplicated, at least 10,000; run them through the verifier and fail on any accept or panic. (3) A `cargo fuzz` target for parse+verify. Include a test-only weakened verifier to prove the corpus catches a missing PAE field. Tests: the four acceptance criteria. Constraints: CI-only tools need not be on the runtime SPDX allowlist but must be listed in `ATTRIBUTION.md`; D2 seeded RNG (counter-based).

## Open questions

- None.

## Children

_Leaf: none._
