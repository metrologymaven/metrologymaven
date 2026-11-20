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
built_in: [fid-solv]
built_pr: 333
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
- `Certificate { kind, active_set, multipliers, kkt, thresholds, cond, is_linear, flags: CertFlags { local_optimum_only }, params, value, algorithm, digest }`; `tie_broken` is returned on `Certified`, outside the digest
- Canonical serialisation (RFC 8785 JSON in the `fid-kci` kernel canonical form, floats in the CORE.JRN.CANON `F64Bits` form `0x` + 16 hex digits) and the SHA-256 digest over those bytes
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
- **Applied at build (KRN.SOLV.CERT.CT, #333 and its review):**
  - *Bytes and digest.* The bytes of `fid.cert@1` are defined by the `fid-kci` kernel canonical form (RFC 8785 JSON restricted to strings, arrays and objects), and the digest is the SHA-256 of those bytes (body without `digest`). `fid-kci` is used instead of `fid-canon` because `fid-canon` is model layer and `fid-solv` is kernel, so nothing is added to `kernel-allowlist.toml`.
  - *Values.* Floats are written in the CORE.JRN.CANON `F64Bits` form, `0x` followed by 16 lowercase hex digits of the IEEE-754 bits, and only that form is parsed. Integers are decimal strings. **Deliberate deviation:** booleans are the strings `"true"`/`"false"`, because the `fid-kci` writer has no JSON booleans.
  - *`tie_broken`.* It is not in the certificate or its digest: the candidate list depends on the solution's slacks, so REQ-003 excludes it. `Certified::tie_broken()` returns it as an in-memory diagnostic (REQ-001 and AC-2 are checked there). `CertFlags` holds only `local_optimum_only`.
  - *Dual thresholds.* The certificate records each active row's `hi||grad g_i||` (`kkt.gradient_norms_hi`). Parse and the self-check recompute every `thr_i = down(down(tol_d * gradient_lo) / ghi_i)` bit for bit (0 when the rounded numerator is not positive, so every `thr_i` is 0 when `gradient_lo` is 0).
  - *Global versus local.* The certificate records `is_linear`, and `steps` must match it. A KKT certificate implies a global optimum only for a linear or convex problem. `local_optimum_only` signals multi-start disagreement that a caller observed.
  - *Signature and errors.* `certify` also takes the `ConicProblem`, which the extraction needs; row `i` of it must be the constraint labelled `row_labels()[i]`. `CertificateFailed` carries the first candidate's KKT report or, when its recomputation or check failed first, that error. `CertError::code()` gives a stable code. The conversion into a `KRN.SOLV` `SolverError` is not built, because the only `SolverError` today is the conic wrapper's, which has no certificate cause.
- **Enclosure obligation (from KRN.SOLV.CERT.RECOMP, #321):** every `ActiveSetProblem` must implement `value_enclosure`, and a nonlinear one also `residual_enclosure` and `jacobian_enclosure`; otherwise recomputation, and so `certify`, fails with `BoundNotProven`. For a linear problem the proven bound applies to the f64 data as given (the Jacobian and residuals at p = 0 are taken as the exact system).

## Children

_Leaf: none._
