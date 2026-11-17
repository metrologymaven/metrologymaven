---
id: KRN.SOLV.CERT.CT
title: Certificate type, certify pipeline and digest rule
level: 4
parent: KRN.SOLV.CERT
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-KRN.SOLV.CERT-003, REQ-KRN.SOLV.CERT-004, REQ-KRN.SOLV.CERT-005, REQ-KRN.SOLV.CERT-006]
covers: []
depends_on: [KRN.SOLV.CERT.AS, KRN.SOLV.CERT.RECOMP, KRN.SOLV.CERT.KKT, KRN.SOLV.CONIC.WRAP, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# KRN.SOLV.CERT.CT: Certificate type, certify pipeline and digest rule

## Purpose

The public face of the certificate module. `certify` takes a converged conic solution and an `ActiveSetProblem`, gets the candidate active sets in tie-break order, recomputes and KKT-checks each in turn, and returns the first one that passes as a `Certificate`, or `certificate_failed`. The certificate holds everything a verifier needs to repeat the check, and its digest is computed from the recomputed values and certificate fields only, never from solver iterates.

## Scope

**In:**
- `certify(problem, solution, config) -> Result<Certified, CertError>` where `Certified { params, value, certificate }`
- Trying candidates in the order from `KRN.SOLV.CERT.AS` and taking the first that recomputes and passes KKT
- `Certificate { kind, active_set, multipliers, kkt, thresholds, cond, flags: CertFlags { tie_broken, local_optimum_only }, params, value, algorithm, digest }`
- Canonical serialisation (RFC 8785 JSON, floats as IEEE-754 hex) and SHA-256 digest through `CORE.JRN.CANON`
- `set_local_optimum_only()` for multi-start callers, which re-derives the digest

**Out** (and which node owns it instead):
- Candidate extraction (`KRN.SOLV.CERT.AS`), recomputation (`KRN.SOLV.CERT.RECOMP`), KKT check (`KRN.SOLV.CERT.KKT`)
- Multi-start agreement that decides `local_optimum_only` (`KRN.SOLV.MSTART`, `KRN.SOLV.SEQ`)
- Verifier rechecking (`TRUST`)

## Requirements

- **REQ-KRN.SOLV.CERT.CT-001** `certify` shall try the candidates in the order given by `KRN.SOLV.CERT.AS`, return the first whose recomputation succeeds and whose KKT report passes, and set `tie_broken` from the extraction.
- **REQ-KRN.SOLV.CERT.CT-002** If no candidate passes, then `certify` shall return `certificate_failed` with the KKT report of the first candidate and the count tried, and no value.
- **REQ-KRN.SOLV.CERT.CT-003** The certificate digest shall be the SHA-256 of the canonical JSON of the certificate's fields other than `digest`, which contain the recomputed params and value, the active set, multipliers from the KKT check, thresholds, tolerances, residuals, flags and algorithm ID, and no field derived from solver iterates.
- **REQ-KRN.SOLV.CERT.CT-004** When `set_local_optimum_only()` is called, the flag shall be set and the digest recomputed, and no other field shall change.
- **REQ-KRN.SOLV.CERT.CT-005** For a frozen set of problems solved from two different starting points that reach the same active set, the certified params, value and digest shall be bit-identical.
- **REQ-KRN.SOLV.CERT.CT-006** The serialised certificate shall have a schema version and shall round-trip (serialise, parse, serialise) to identical bytes.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/solv/cert/ct-v1` (60 minimum-zone and envelope problems with test `ActiveSetProblem` implementations, each solved from two seeded conic starting points), when `certify` runs on both solutions, then params, value and digest are bit-identical for every pair whose active sets agree.
- **AC-2** (tie) Given AC-2 of `KRN.SOLV.CERT.AS` (two candidates), then the certificate uses {0,1,4} and `tie_broken = true`.
- **AC-3** (error path) Given a solution whose candidates all fail KKT, then `certificate_failed` with the count tried.
- **AC-4** (iterate exclusion) Given two solutions that differ only in their x, y and s bits beyond the thresholds' resolution, then the two certificates' digests are equal.
- **AC-5** Given a certificate, then serialise-parse-serialise gives identical bytes and the digest matches a recomputation from the parsed fields; after `set_local_optimum_only()` only the flag and digest differ.

## Design

- **Registry command(s):** none; library API `fid_solv::cert::{certify, Certified, Certificate, CertFlags, CertKind, CertConfig}`.
- **Data structures:** `Certificate` as listed in Scope, schema `fid.cert@1`; `CertKind::{Chebyshev, Envelope, Tangential, Zone}` supplied by the caller; `CertConfig { thresholds, recomp: RecompConfig, kkt: KktTolerances }`.
- **Algorithm:** extract candidates; loop in order: recompute, KKT check, first pass wins; build the certificate; serialise with `CORE.JRN.CANON` typed values (floats as `F64Bits`); digest with its SHA-256.
- **Errors:** `CertError::{CertificateFailed { tried, first_report }, ActiveSetUndetermined, ..}`, convertible to the `KRN.SOLV` `SolverError`.
- **Determinism:** D0.
- **Performance budget:** certify overhead beyond the solves < 0.2 ms for a 7-parameter problem with one candidate [unverified until measured].
- **Crate:** `fid-solv` (module `cert`).

## Test fixtures

`fixtures/solv/cert/ct-v1`: committed seeded generator producing problems and two conic starting points each; CC-BY-4.0. Uses `fixtures/solv/cert/as-v1` for the tie case.

## Build prompt

> In crate `fid-solv`, module `cert`, implement `certify(problem: &dyn ActiveSetProblem, sol: &ConicSolution, kind: CertKind, cfg: &CertConfig) -> Result<Certified, CertError>`: call `cert::active::extract` with the problem's labels and expected size, then for each candidate in order call `cert::recomp::recompute` and `cert::kkt::check`; the first that passes yields `Certificate { kind, active_set, multipliers (from KKT), kkt, thresholds, cond, flags { tie_broken, local_optimum_only: false }, params, value, algorithm, digest }`. None passes: `CertificateFailed { tried, first_report }`. Digest: SHA-256 via the `fid-canon` crate (CORE.JRN.CANON) over RFC 8785 JSON of all fields except `digest`, floats as IEEE-754 hex; never include x, y, s or iteration counts. `set_local_optimum_only()` sets the flag and recomputes the digest. Schema `fid.cert@1` with round-trip parse. Tests: `fixtures/solv/cert/ct-v1` (two-start bit identity), the tie case, all-fail case, iterate-exclusion case, round-trip. Constraints (CONSTITUTION §4 KNP-1, §8): no FMA, no HashMap/HashSet, no `unsafe`, no I/O; deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`. Acceptance: AC-1..AC-5 of spec node KRN.SOLV.CERT.CT.

## Open questions

- The certificate schema must match what `TRUST.VERIFY` rechecks; agree on `fid.cert@1` when TRUST's recheck leaf is written. The verifier needs the problem data (points) too, which the ResultRecord's input digests reference.
- Depending on `CORE.JRN.CANON` is layer-legal (L0 to L0) but brings its SHA-256 dependency into the kernel closure; it must be on `kernel-allowlist.toml`.

## Children

_Leaf: none._
