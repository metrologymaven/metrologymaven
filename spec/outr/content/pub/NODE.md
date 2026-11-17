---
id: OUTR.CONTENT.PUB
title: Content manifest and publication check
level: 3
parent: OUTR.CONTENT
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT-001, REQ-OUTR.CONTENT-008]
covers: []
depends_on: [OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.CONTENT.PUB: Content manifest and publication check

## Purpose

The content manifest that every tutorial, migration guide and video carries, and the check that must pass before any of them is published: every required manifest field present and well-formed, the named journal file present with a matching digest, and the OUTR.BRAND claim lint clean. It is the shared gate the other CONTENT children publish through. It is project tooling, not a product registry command.

## Scope

**In:** the manifest schema (front matter of each content source); the `content-check` command that validates manifests, journal digests and runs the claim lint; its report and exit codes.
**Out:** replay of journals (`OUTR.CONTENT.REPRO`); the lint rules (`OUTR.BRAND.LINT`); writing the content (`OUTR.CONTENT.TUT`, `OUTR.CONTENT.MIG`, `OUTR.CONTENT.VIDEO`); guide registration in the portal (`OUTR.DOCS.FRAME`).

## Requirements

- **REQ-OUTR.CONTENT.PUB-001** Every content source shall start with a manifest giving `kind` (tutorial, migration or video), `example_project` (dataset ID), `journal` (relative path), `journal_sha256`, and `verified_release` (release tag).
- **REQ-OUTR.CONTENT.PUB-002** If a content source has no manifest, a missing field, or a field that does not match its format, then the check shall report an error naming the file and the field.
- **REQ-OUTR.CONTENT.PUB-003** If the journal file named in a manifest does not exist or its SHA-256 digest differs from `journal_sha256`, then the check shall report an error naming the file and the journal.
- **REQ-OUTR.CONTENT.PUB-004** The check shall run the OUTR.BRAND claim lint on every content source and report its errors with the lint's rule IDs.
- **REQ-OUTR.CONTENT.PUB-005** The check shall exit 0 with no errors, 1 with any error, and 2 when the content root or the lint rule file cannot be read.
- **REQ-OUTR.CONTENT.PUB-006** The check shall write a JSON report sorted by file and field, byte-identical across runs on the same inputs.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/content/ok` (one tutorial, one migration guide, one video source, each with a complete manifest and a journal file whose digest matches), when `fid-outreach content-check --root fixtures/outr/content/ok --rules claim-rules.toml --json` runs, then it exits 0 with zero findings.
- **AC-2** Given fixture `fixtures/outr/content/missing-field` (tutorial without `verified_release`), when the check runs, then it exits 1 with one finding `missing_field` naming the file and `verified_release`.
- **AC-3** Given fixture `fixtures/outr/content/bad-digest` (journal changed after the digest was recorded), when the check runs, then it exits 1 with one finding `journal_digest_mismatch`.
- **AC-4** Given fixture `fixtures/outr/content/no-journal` (manifest names a journal path that does not exist), when the check runs, then it exits 1 with `journal_missing`.
- **AC-5** Given fixture `fixtures/outr/content/lint-fail` (a tutorial sentence "This workflow is certified."), when the check runs, then it exits 1 with one finding carrying lint rule ID CL001.
- **AC-6** (error path) Given a non-existent `--root`, when the check runs, then it exits 2 and writes no report.
- **AC-7** (determinism) Given fixture `ok`, when the check runs twice, then the two JSON reports are byte-identical.

## Design

- **Command:** `fid-outreach content-check --root <dir> --rules <file> [--bench-prefix <url>]... [--json]`. Project tooling; nearest class R0.
- **Manifest:** YAML-style front matter at the top of each `.md` source (videos use a sidecar `video.md`). Formats: `kind` in {tutorial, migration, video}; `example_project` matches `[a-z0-9-]{1,64}`; `journal` is a relative path inside `--root` (no `..`, no absolute path); `journal_sha256` is 64 lowercase hex characters; `verified_release` matches the release tag pattern of ECO.REL (initially `v<semver>`).
- **Algorithm:** walk `--root` in sorted order for `.md` files; parse front matter with a bounded parser; validate fields; resolve and hash journals (streamed SHA-256; files over 1 GB refused); call the `claim_lint` module of the same crate with the given rules; merge findings; sort; write RFC 8785 JSON.
- **Errors:** `RootUnreadable`, `RulesParse`, findings `no_manifest`, `missing_field`, `bad_field`, `journal_missing`, `journal_digest_mismatch`, `path_escape`, plus lint findings.
- **Determinism:** D0.
- **Performance budget:** 500 content items with 2 GB of journals in under 30 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach`, module `content_check` (reuses module `claim_lint` from OUTR.BRAND.LINT).

## Test fixtures

Synthetic fixtures under `fixtures/outr/content/` (`ok`, `missing-field`, `bad-digest`, `no-journal`, `lint-fail`) with small hand-made journal files (any bytes; only digests matter here), CC-BY-4.0.

## Build prompt

> Build the `content-check` subcommand of the Rust crate `fid-outreach`: `fid-outreach content-check --root <dir> --rules <file> [--bench-prefix <url>]... [--json]`. It is outreach tooling, not part of the product kernel or registry. The same crate already has a `claim_lint` module (the outreach claim lint); call it, do not reimplement it.
>
> Purpose: every tutorial, migration guide and video description is a Markdown file whose front matter is a manifest with `kind` (tutorial | migration | video), `example_project` (`[a-z0-9-]{1,64}`), `journal` (relative path inside the root; reject `..` and absolute paths with `path_escape`), `journal_sha256` (64 lowercase hex), `verified_release` (`v` + semver). The check validates every manifest, hashes each journal (streamed SHA-256, refuse files over 1 GB), and runs the claim lint on each file.
>
> Findings: `no_manifest`, `missing_field`, `bad_field`, `journal_missing`, `journal_digest_mismatch`, `path_escape`, and lint findings with their rule IDs; each with file and field or line. Exit 0 with no findings, 1 with any, 2 if the root or rules file cannot be read (no report then). Report: RFC 8785 canonical JSON sorted by file then field.
>
> Acceptance tests with fixtures under `fixtures/outr/content/`: `ok` exits 0; `missing-field` reports `missing_field` for `verified_release`; `bad-digest` reports `journal_digest_mismatch`; `no-journal` reports `journal_missing`; `lint-fail` ("This workflow is certified.") reports CL001; a missing root exits 2; two runs on `ok` give byte-identical reports.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing, fail closed; no network; BTreeMap only; allowlisted dependency licences.

## Open questions

- The release tag pattern belongs to ECO.REL; `v<semver>` is assumed until it is expanded.
- Dataset IDs for example projects belong to ECO.DATA; the pattern above is a placeholder.

## Children

_Leaf: none._
