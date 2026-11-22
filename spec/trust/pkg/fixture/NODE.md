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
built_in: [fid-pkg]
built_pr: 423
satisfies: [REQ-TRUST.PKG-001, REQ-TRUST.PKG-007, REQ-TRUST.PKG-008]
covers: []
depends_on: [TRUST.PKG.MANIFEST, TRUST.SIGN.DSSE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-21
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

- Test signing keys (settled 2026-11-21): the two-signature golden is signed with the keys `TRUST.SIGN.DSSE` already publishes in `fixtures/dsse/keys` (the RFC 8032 section 7.1 TEST 1 Ed25519 seed and the RFC 6979 A.2.5 P-256 key). No new key is generated or committed.

## Build notes (2026-11-21)

Built as stdlib Python generators and checks in `fixtures/pkg/`, the spec text `docs/result-package/v1.md`, the schema `schema/statement-1.json`, and in crate `fid-pkg` the reference reader binary `fid-pkg-conformance` with `tests/golden.rs`; `crates/fid-sign-core/tests/golden_packages.rs` checks the two signatures with the real checker.

**Package content decision.** `TRUST.PKG.CONTENT.*` is not built, so the goldens follow the content model `TRUST.REPLAY.SELECT` (PR #421) assumed, so that its expected plans hold for the goldens unchanged (checked: #421's reference `select()` on `golden/points.zip` and `golden/results.zip` gives its `points.plan.json` and `results.plan.json` byte for byte):

- `results/records.json`: RFC 8785 array of ResultRecords sorted by `id` bytes. The golden records are a superset of the fields #421 reads (`id`, `evaluation_basis`, `algorithm`, `journal_entry`): every CONSTITUTION section 5 minimum field, with `uncertainty` `{class, method, expanded{U,k}, unit}` (UEC-2) or exactly `{"class":"UEC-0","status":"not_evaluated"}` (REQ-TRUST.PKG.CONTENT.SELECT-003), `decision {rule, outcome}`, `spec {standards, defaults_profile}`.
- `journal/segment.jsonl`: one RFC 8785 line `{"header", "payload"?}` per entry, header hash-chained, starting at the session header (no checkpoint, so no `journal/checkpoints/` member). Journal, points and kernel manifest of `points.zip` are byte-identical to #421's stand-in `fixtures/replay/select/points.zip`.
- `points/<sha256 hex>.pts`, media type `text/plain`, named by the digest of the bytes. `TRUST.PKG.CONTENT.EVID`'s design names `.fpts` [unverified]; the golden keeps #421's `.pts` until the point-set format is fixed.
- `results/results.qif` is a labelled placeholder (`<QIFDocument/>` with a comment) until `IO.RES.QIFR.ROUT` defines it.
- Difference from #421's stand-ins: `golden/results.zip` (and `features.zip`) also carry the journal segment that produced the records (REQ-TRUST.PKG-005), and every golden carries `results/results.qif`. Neither changes #421's plans.

If the CONTENT leaves choose other shapes, `make_golden.py` follows them and the goldens are re-pinned in that PR.

**Deviations from the Design, stated openly:**

- The generator is not a `fid-pkg` xtask built on `fid-pkg::manifest`: the goldens must come from an independent reference, so `fixtures/pkg/make_golden.py` builds them with the TRUST.PKG.MANIFEST stdlib reference writer `make_fixtures.py`, and `fixtures/pkg/generate` is the one entry point (`--check`, `--self-test`) over `make_fixtures.py`, `make_reader_fixtures.py` and `make_golden.py`. The error it reports is `fixture.mismatch <path>`.
- No `fixtures/pkg/seeds.toml`: nothing is random; all content is fixed in the generators.
- The malformed corpus is the one `TRUST.PKG.READER` built (`make_reader_fixtures.py`, each mutation documented in its `expect.json` `note`), assembled byte by byte rather than as mutations of these goldens; this leaf adds the missing `cd_signature` case (106 cases). It covers every reader code except `pkg.io` and every reason except `source_changed`, `package_size` (16 GiB), `index_budget` and `buffer_cap`, which cannot be a static file and are covered by the reader's own tests.
- Two signatures are two envelopes, `signatures/0.dsse.json` (P-256) and `signatures/1.dsse.json` (Ed25519), each over a signature statement whose single subject is `statement.json` (the multi-signer form `TRUST.SIGN.TROOT` checks), not one envelope with two signatures.
- The predicate schema is `schema/statement-1.json` (the whole statement with its predicate).
- The run-conformance protocol: the reader is called as `READER ARG ... <package>` and prints `{"result":"accepted"}` or `{"result":"rejected","error_code":...,"reason":...}`.
- The drift check (`fixtures/pkg/check_spec_drift.py`) also compares reason tags and manifest/statement fields, not only error codes.
- Performance: full regeneration and check take about 3 s locally.

## Children

_Leaf: none._
