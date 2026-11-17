---
id: TRUST.ESIG.BIND
title: Signature statement and signing journal events
level: 3
parent: TRUST.ESIG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.ESIG-003, REQ-TRUST.ESIG-004]
covers: []
depends_on: [TRUST.PKG.MANIFEST, TRUST.ESIG.MEANING]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: [sign.sign_package@1, sign.refuse@1]
updated: 2026-11-16
---

# TRUST.ESIG.BIND: Signature statement and signing journal events

## Purpose

Defines the exact bytes a human signs and the journal events that record it. A human signature is a DSSE envelope over a **signature statement**: an in-toto Statement v1 whose single subject is the package's `statement.json` digest and whose predicate carries the manifestation (printed name, UTC time with local offset, meaning, key fingerprint, user-verification method) plus the sequence links that `TRUST.ESIG.MULTI` validates. This leaf builds and parses that statement and the `sign.sign_package@1` and `sign.refuse@1` event bodies, so ceremony, verifier and journal all agree on one shape.

## Scope

**In:**
- Signature predicate schema and type constant
- Builder (from ceremony inputs) and bounded parser (for the verifier)
- Event body schemas for `sign.sign_package@1` and `sign.refuse@1`
- Canonical serialisation (RFC 8785) and digests

**Out:**
- Sequence validation across several signatures (`TRUST.ESIG.MULTI`)
- Producing the DSSE envelope (`TRUST.SIGN.DSSE` via `TRUST.SIGN.UV`)
- Running the ceremony (`TRUST.ESIG.CEREMONY`)

## Requirements

- **REQ-TRUST.ESIG.BIND-001** The signature statement shall be an in-toto Statement v1 with exactly one subject, named `statement.json`, whose `sha256` is the package statement digest.
- **REQ-TRUST.ESIG.BIND-002** The signature predicate shall contain `signer_name`, `signed_at` (RFC 3339 UTC), `local_offset` (`±HH:MM`), `meaning_code`, `meaning_label`, `key_fingerprint`, `uv_method`, `sequence_index` and `prior` (list of `{sequence_index, meaning_code, signature_statement_sha256}`), and the builder shall fail with `esig.field_missing` if any is absent or empty.
- **REQ-TRUST.ESIG.BIND-003** The builder shall serialise the signature statement as RFC 8785 canonical JSON, so that the same inputs always yield the same bytes.
- **REQ-TRUST.ESIG.BIND-004** The `sign.sign_package@1` event body shall hold the package statement digest, the signature statement digest, meaning code, sequence index and key fingerprint, and its result digest shall equal the signature statement digest.
- **REQ-TRUST.ESIG.BIND-005** The `sign.refuse@1` event body shall hold the package statement digest, the ceremony step reached (`adoption | preview | confirm | uv | write`) and a reason code, and shall hold no signature bytes.
- **REQ-TRUST.ESIG.BIND-006** The parser shall reject a signature statement over 1 MiB, with unknown predicate fields, or with a `meaning_code` that fails the catalogue syntax, with `esig.malformed`.
- **REQ-TRUST.ESIG.BIND-007** The signer's printed name shall be stored as a detachable journal field (salted commitment in the header), while it stays in clear inside the signature statement.

## Acceptance criteria

- **AC-1** Given inputs in `fixtures/esig/bind/approve-1.json`, when the builder runs twice, then both outputs are byte-identical and equal `fixtures/esig/bind/approve-1.statement.json`.
- **AC-2** Given that statement, when parsed, then every field round-trips and the subject digest equals the digest of `fixtures/pkg/golden/points.zip`'s `statement.json`.
- **AC-3** Given the event builder on AC-1, then the `sign.sign_package@1` result digest equals SHA-256 of the AC-1 bytes.
- **AC-4** (error path) Given inputs with an empty `signer_name`, then the builder fails with `esig.field_missing`; given a statement with an extra predicate field, then the parser fails with `esig.malformed`.
- **AC-5** (refuse) Given a refusal at step `uv` with reason `uv_cancelled`, then the event body validates against `schema/sign-refuse-1.json` and contains no signature bytes.

## Design

- **Registry command(s):** none; library used by `sign.sign_package@1` and the verifier.
- **Data structures:** `SignaturePredicate { signer_name, signed_at, local_offset, meaning_code, meaning_label, key_fingerprint, uv_method, sequence_index: u8, prior: Vec<PriorRef> }`; `PriorRef { sequence_index, meaning_code, signature_statement_sha256 }`; `SignPackageEvent`, `RefuseEvent`.
- **Algorithm:** build predicate → wrap in Statement v1 → RFC 8785 → SHA-256. Time values come from the ceremony (clock read once, journaled), never from this library.
- **Errors:** `esig.field_missing`, `esig.malformed`.
- **Determinism:** D0 given inputs.
- **Performance budget:** trivial.
- **Crate:** `fid-esig` (module `bind`); parser also compiled into `fid-verify`.

## Test fixtures

- `fixtures/esig/bind/*`: synthetic inputs and expected statements; schemas `schema/sign-predicate-1.json`, `schema/sign-package-1.json`, `schema/sign-refuse-1.json`.

## Build prompt

> In crate `fid-esig`, add module `bind`. Build and parse the human **signature statement**: in-toto Statement v1 with exactly one subject `statement.json` (sha256 = package statement digest) and a signature predicate with `signer_name`, `signed_at` (RFC 3339 UTC), `local_offset`, `meaning_code`, `meaning_label`, `key_fingerprint`, `uv_method`, `sequence_index`, `prior[]` (`sequence_index`, `meaning_code`, `signature_statement_sha256`). Serialise with RFC 8785; reject unknown fields, oversize (1 MiB) or bad meaning syntax (`esig.malformed`); missing or empty fields fail with `esig.field_missing`. Also build the journal event bodies for `sign.sign_package@1` (result digest = signature statement digest) and `sign.refuse@1` (step and reason, no signature bytes); mark `signer_name` as a detachable journal field. Publish JSON Schemas for all three. The predicate type URI is one crate constant under the internal working name. Tests: the five acceptance criteria. Constraints: CONSTITUTION §8, D0, the module must build for wasm32 for the verifier.

## Open questions

- The manifestation fields follow the concept of 21 CFR 11.50 [unverified]; `TRUST.PROFILE.MAP` holds the section-number mapping.

## Children

_Leaf: none._
