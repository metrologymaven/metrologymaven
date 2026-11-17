---
id: TRUST.PKG.MANIFEST
title: Package container layout, manifest and in-toto statement
level: 3
parent: TRUST.PKG
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG-001, REQ-TRUST.PKG-002, REQ-TRUST.PKG-008]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# TRUST.PKG.MANIFEST: Package container layout, manifest and in-toto statement

## Purpose

Defines the byte-level shape of a Result Package: the container, the fixed member layout, the `manifest.json` schema with its semantic format version, and the in-toto Statement that a DSSE envelope signs. It is a pure library (types, canonical serialiser, schema) that the exporter writes with and every verifier-side check reads with. Keeping it a leaf on its own makes the public contract one small, reviewable crate module.

## Scope

**In:**
- Container: ZIP with zip64, deterministic member order and fixed metadata
- Member layout (reserved top-level folders) and path rules
- `manifest.json` schema (JSON Schema 2020-12) and the Rust types
- Semantic format version and compatibility rule
- The in-toto Statement v1 written to `statement.json`, its subjects and the predicate
- Canonical serialisation (RFC 8785) and SHA-256 member digests

**Out:**
- Choosing which results, points and journal entries go in (`TRUST.PKG.CONTENT`)
- Writing the package to an export root (`TRUST.PKG.EXPORT`)
- Bounded reading and limit enforcement (`TRUST.PKG.READER`)
- Golden and malformed packages, published spec page (`TRUST.PKG.FIXTURE`)
- DSSE envelopes in `signatures/` (`TRUST.SIGN`)

## Requirements

- **REQ-TRUST.PKG.MANIFEST-001** The manifest shall carry `format` equal to the fixed package format name and `format_version` as a semantic version string, starting at `1.0.0`.
- **REQ-TRUST.PKG.MANIFEST-002** The manifest shall list every member except `statement.json` and members under `signatures/`, each with path, media type, byte length as a decimal string and lowercase hex SHA-256, sorted by the UTF-8 bytes of the path.
- **REQ-TRUST.PKG.MANIFEST-003** The statement shall be an in-toto Statement v1 whose subjects are `manifest.json` plus every manifest member, each named by path with a `sha256` digest, and whose predicate type is a single constant of the crate [unverified: final URI after D-002 rename].
- **REQ-TRUST.PKG.MANIFEST-004** The serialiser shall write `manifest.json` and `statement.json` as RFC 8785 canonical JSON with no trailing newline.
- **REQ-TRUST.PKG.MANIFEST-005** The container writer shall place `manifest.json` first, then all other members in manifest order, then `statement.json`, with every entry timestamp set to 1980-01-01 00:00:00, no extra fields other than zip64, and no comments.
- **REQ-TRUST.PKG.MANIFEST-006** A member path shall be relative, use `/` separators, be Unicode NFC, contain no `.` or `..` segment, no backslash, no NUL and no drive prefix, and be at most 4096 bytes; the path validator shall reject any other path with a typed error.
- **REQ-TRUST.PKG.MANIFEST-007** When the same logical contents are serialised twice, on any supported platform, the manifest, statement and container bytes shall be identical.
- **REQ-TRUST.PKG.MANIFEST-008** A reader of format major version N shall accept any minor version of N, and shall reject a different major version with error `pkg.unsupported_version`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/pkg/contents/minimal.json` (three members), when the serialiser builds a package twice, then the two container files are byte-identical and their SHA-256 equals the value pinned in `fixtures/pkg/golden/minimal.zip.sha256`.
- **AC-2** Given the package from AC-1, when unpacked with the system `unzip` (Info-ZIP) and with Python's `zipfile`, then both list exactly the members in the manifest plus `statement.json`, in that order.
- **AC-3** Given the package from AC-1, when `manifest.json` is validated against `schema/manifest-1.json`, then it is valid; and when each member is hashed independently with `sha256sum`, every digest matches its manifest entry.
- **AC-4** Given `statement.json` from AC-1, when parsed by a pinned in-toto reference implementation, then it is accepted as a Statement v1 with the expected subject count.
- **AC-5** (error path) Given each path in `fixtures/pkg/paths/bad.txt` (e.g. `../x`, `a\\b`, `/abs`, `C:x`, a non-NFC name, a 4097-byte name), when the path validator runs, then it returns `pkg.bad_path` for every one and never panics.
- **AC-6** (version) Given manifests with `format_version` `1.4.0` and `2.0.0`, when read by a 1.x reader, then the first is accepted and the second fails with `pkg.unsupported_version`.
- **AC-7** (cross-platform) Given AC-1 run in CI on Windows, Linux and macOS, then all three container digests are equal.

## Design

- **Registry command(s):** none; library only.
- **Data structures:** `Manifest { format, format_version: SemVer, created_by, evidence_tier: EvidenceTier, profile_digest: Sha256, kernel_manifest_digest: Sha256, members: Vec<Member> }`; `Member { path: PkgPath, media_type, length: u64, sha256: Sha256 }`; `PkgPath` (validated newtype); `Statement { _type, subject: Vec<Subject>, predicate_type, predicate: PackagePredicate }`; `PackagePredicate { manifest_sha256, format_version, result_count, evidence_tier }`.
- **Layout:** `manifest.json`; `results/records.json` (ResultRecords); `results/results.qif` (QIF Results); `journal/segment.jsonl`; `journal/checkpoints/<index>.json`; `points/<sha256>.<ext>`; `kernel/manifest.json`; `profile/profile.json`; `statement.json`; `signatures/<n>.dsse.json` (added after export).
- **Algorithm:** build members, sort by path bytes, hash each (streaming SHA-256), canonicalise manifest (RFC 8785), build statement, canonicalise, write ZIP (APPNOTE zip64 layout; method 0 stored for already-compressed members, method 8 deflate at fixed level otherwise, chosen by media type only so output is deterministic).
- **Errors:** `pkg.bad_path`, `pkg.duplicate_path`, `pkg.unsupported_version`, `pkg.schema_invalid`, `pkg.io`.
- **Determinism:** D0. No clock, no locale, no hash-map iteration; deflate implementation pinned (a pure-Rust deflate crate at a fixed version and level).
- **Performance budget:** hashing at no less than 500 MB/s single-threaded on reference hardware [unverified until measured].
- **Crate:** `fid-pkg` (module `manifest`, `container`).

## Test fixtures

- `fixtures/pkg/contents/*.json`: synthetic member lists with small inline bodies (generated, CC-BY-4.0).
- `fixtures/pkg/golden/*.sha256`: pinned digests, regenerated only by an explicit fixture-update PR.
- `fixtures/pkg/paths/bad.txt` and `good.txt`: path-rule cases.
- `schema/manifest-1.json`: the published JSON Schema (also used by `TRUST.PKG.FIXTURE`).

## Build prompt

> In crate `fid-pkg`, implement modules `manifest` and `container` for the Result Package format version 1.0.0. Provide the types `Manifest`, `Member`, `PkgPath`, `Statement` and `PackagePredicate`, a canonical JSON serialiser following RFC 8785, streaming SHA-256 member hashing, and a deterministic ZIP (zip64) writer. Layout: `manifest.json` first, other members sorted by UTF-8 path bytes, `statement.json` last; timestamps fixed at 1980-01-01 00:00:00; no comments or extra fields except zip64; compression method chosen from media type only. The manifest lists every member except `statement.json` and `signatures/*`; the statement is an in-toto Statement v1 (`_type` `https://in-toto.io/Statement/v1`) whose subjects are `manifest.json` plus every member, with predicate type taken from one crate constant (placeholder under the internal working name, renamed globally at publication). Validate paths (relative, `/`, NFC, no `.`/`..`, no backslash, NUL or drive prefix, at most 4096 bytes) and return typed errors `pkg.bad_path`, `pkg.duplicate_path`, `pkg.unsupported_version`, `pkg.schema_invalid`, `pkg.io`. Readers accept the same major version and reject others. Publish `schema/manifest-1.json` (JSON Schema 2020-12). Acceptance tests: byte-identical double serialisation and pinned digest (`fixtures/pkg/golden/minimal.zip.sha256`), interop listing with Info-ZIP `unzip` and Python `zipfile`, schema validation, independent `sha256sum` check, bad-path corpus, version acceptance, and equal digests on Windows, Linux and macOS CI. Constraints: no panics, no `unwrap`/`expect`, no `unsafe`; no std `HashMap`/`HashSet` in serialised paths; determinism D0; every dependency on the SPDX allowlist.

## Open questions

- The predicate type URI is a placeholder until D-002 picks the public name; one constant keeps the rename trivial.
- Whether a `points/` member may be split across several files above a size threshold is left to `TRUST.PKG.CONTENT`.

## Children

_Leaf: none._
