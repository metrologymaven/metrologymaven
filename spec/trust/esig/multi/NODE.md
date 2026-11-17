---
id: TRUST.ESIG.MULTI
title: Ordered multi-signer sequences
level: 3
parent: TRUST.ESIG
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.ESIG-007]
covers: []
depends_on: [TRUST.ESIG.BIND]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.ESIG.MULTI: Ordered multi-signer sequences

## Purpose

Lets several people sign one package in order (for example author, then reviewer, then approver), with each signature committing to the same package statement and to every earlier signature. This leaf validates a sequence of signature statements: contiguous indices, correct prior links, and an optional required order from the profile. The ceremony uses it before appending, and the verifier uses it to report the sequence.

## Scope

**In:**
- Sequence model and validation of `sequence_index` and `prior` links
- Optional required order (list of meaning codes) supplied by the caller
- Next-signature preparation: computing the `prior` list for the next signer
- Limit of 8 signatures in format v1

**Out:**
- Statement shape (`TRUST.ESIG.BIND`)
- Where the required order comes from (`TRUST.PROFILE.DEFS`)
- Cryptographic validity of each envelope (`TRUST.SIGN.TROOT`)

## Requirements

- **REQ-TRUST.ESIG.MULTI-001** A valid sequence shall have `sequence_index` values 0..n-1 with no gaps or repeats, with n at most 8.
- **REQ-TRUST.ESIG.MULTI-002** Signature k shall list in `prior` exactly the signatures 0..k-1, each with its meaning code and the SHA-256 of its signature statement, and the validator shall report `esig.sequence_broken` with k otherwise.
- **REQ-TRUST.ESIG.MULTI-003** Every signature statement in a sequence shall have the same subject digest, and the validator shall report `esig.subject_mismatch` otherwise.
- **REQ-TRUST.ESIG.MULTI-004** When a required order of meaning codes is given, the validator shall report `esig.order_violation` if the sequence's meaning codes are not a prefix of that order.
- **REQ-TRUST.ESIG.MULTI-005** When asked to prepare signature n, the module shall return the `prior` list for it, or `esig.sequence_full` when n equals 8.

## Acceptance criteria

- **AC-1** Given `fixtures/esig/multi/three-ok/` (AUTHORED, REVIEWED, APPROVED), when validated with required order `[AUTHORED, REVIEWED, APPROVED, RELEASED]`, then the result is valid with three entries.
- **AC-2** (error path) Given `fixtures/esig/multi/gap/` (indices 0, 2), then `esig.sequence_broken` at 2; given `wrong-prior/` (prior digest altered), then `esig.sequence_broken` at 1.
- **AC-3** (error path) Given `fixtures/esig/multi/two-subjects/`, then `esig.subject_mismatch`; given `three-ok` with required order `[REVIEWED, ...]`, then `esig.order_violation`.
- **AC-4** Given a valid sequence of 8, when preparing signature 8, then `esig.sequence_full`.

## Design

- **Registry command(s):** none.
- **Data structures:** `Sequence(Vec<SignaturePredicateWithDigest>)`; `RequiredOrder(Vec<MeaningCode>)`.
- **Algorithm:** sort by `sequence_index`; walk once checking contiguity, subject equality and prior lists against computed digests; compare meaning prefix.
- **Errors:** `esig.sequence_broken`, `esig.subject_mismatch`, `esig.order_violation`, `esig.sequence_full`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-esig` (module `multi`); also compiled into `fid-verify`.

## Test fixtures

- `fixtures/esig/multi/*`: sequences built with `TRUST.ESIG.BIND` from published test keys (generated, CC-BY-4.0).

## Build prompt

> In crate `fid-esig`, add module `multi`. Validate a sequence of human signature statements (from module `bind`): indices 0..n-1 contiguous, n ≤ 8; signature k's `prior` lists exactly signatures 0..k-1 with meaning codes and SHA-256 of their signature statements; all subjects equal; optional required order of meaning codes must have the sequence as a prefix. Provide `prepare_prior(sequence)` for the next signer, failing with `esig.sequence_full` at 8. Errors: `esig.sequence_broken` (with index), `esig.subject_mismatch`, `esig.order_violation`, `esig.sequence_full`. Tests: the four acceptance criteria with fixtures in `fixtures/esig/multi/`. Constraints: CONSTITUTION §8, D0, builds for wasm32.

## Open questions

- Parallel (unordered) co-signing is not supported in v1; add by a minor format version if users need it.

## Children

_Leaf: none._
