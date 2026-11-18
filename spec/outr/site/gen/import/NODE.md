---
id: OUTR.SITE.GEN.IMPORT
title: Generated-input lock file, fetch, digest check and build manifest
level: 4
parent: OUTR.SITE.GEN
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 123
satisfies: [REQ-OUTR.SITE.GEN-004, REQ-OUTR.SITE.GEN-005]
covers: []
depends_on: []
uses_artifacts: [ECO.REL.PROC.CHANNEL, ECO.PARITY.GEN, ECO.PARITY.STATUS, ECO.APPS.LIST, ECO.COMM.SCOPE, OUTR.LAUNCH.PLAN, UX.TOKENS, OUTR.BRAND.ASSET]
side_effect: [W1, W1]
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.GEN.IMPORT: Generated-input lock file, fetch, digest check and build manifest

## Purpose

Makes every generated input of the website (parity and status reports, release manifests, the app list, spec-tree reports, launch-plan milestones, design tokens and brand assets) an exact, pinned, verified file. Inputs are vendored into the site repository under `inputs/` and pinned by SHA-256 in `inputs.lock.toml`, so the build itself never touches the network; a separate fetch command adds a new release's files in a reviewable pull request. The verified set is described in `build-manifest.json`, which the build publishes.

## Scope

**In:**
- `inputs.lock.toml` format: one entry per input with name, kind, release tag or source revision, path and SHA-256
- `hosts.toml`: the project-controlled host allowlist (read also by `OUTR.SITE.DL` and by `fetch`)
- `fid-outreach site import fetch`: downloads one release's manifest and report assets from an allowlisted host into `inputs/<tag>/` and updates the lock file
- `verify_and_stage` (library function, also run as `fid-outreach site import verify`): digest checks, cross-check against each release manifest, staging into a read-only data folder
- `build-manifest.json` content

**Out:**
- Running the build and copying the manifest into the output (`OUTR.SITE.GEN.BUILD`)
- Generating any of the inputs (`ECO.PARITY.GEN`, `ECO.PARITY.STATUS`, `ECO.REL.PROC.CHANNEL`, `ECO.APPS.LIST`, `ECO.COMM.SCOPE`, `OUTR.LAUNCH.PLAN`, `UX.TOKENS`, `OUTR.BRAND.ASSET`)
- Verifying DSSE signatures of release manifests (`TRUST` and `fid-verify`; the lock file pins the manifest digest instead, reviewed in the pull request)
- Checking download links against the allowlist (`OUTR.SITE.DL`)

## Requirements

- **REQ-OUTR.SITE.GEN.IMPORT-001** The lock file shall list every generated input with `name`, `kind` (one of `release_manifest`, `release_asset`, `spec_report`, `app_list`, `launch_plan`, `tokens`, `brand_asset`), `release` (tag, optional), `source_rev` (commit, optional; at least one of the two present), `date` (`YYYY-MM-DD`: the release date from the release manifest for release inputs, otherwise the commit date of `source_rev`), `path` under `inputs/` and `sha256`, and the verify step shall reject any other kind or an entry with neither `release` nor `source_rev`.
- **REQ-OUTR.SITE.GEN.IMPORT-002** If a file listed in the lock file is missing, or its SHA-256 differs from the lock entry, then the verify step shall exit 1 and name the input and both digests.
- **REQ-OUTR.SITE.GEN.IMPORT-003** If a `release_asset` entry's file name is absent from the artefact list of the same release's staged `release-manifest.json`, or its SHA-256 differs from the digest that manifest records, then the verify step shall exit 1 and name the input and the release.
- **REQ-OUTR.SITE.GEN.IMPORT-004** If a file exists under `inputs/` that no lock entry names, then the verify step shall exit 1 and name the file.
- **REQ-OUTR.SITE.GEN.IMPORT-005** When verification succeeds, the verify step shall write `build-manifest.json` as RFC 8785 canonical JSON with the site revision, the revision date and, for each input sorted by path, its name, path, kind, release, source revision, date and SHA-256 (absent optional fields omitted).
- **REQ-OUTR.SITE.GEN.IMPORT-006** When `fetch --release <tag> --base-url <url>` runs, it shall download only from a URL whose host is listed in `hosts.toml`, fetch `release-manifest.json` and the release's `parity.json`, `parity.md`, `status.json` and `status.md` when the manifest lists them, verify each against the manifest's SHA-256, write them under `inputs/<tag>/` and add or replace their lock entries.
- **REQ-OUTR.SITE.GEN.IMPORT-007** If the fetch URL's host is not in `hosts.toml`, or a fetched file fails its manifest digest, then fetch shall exit 1, write nothing under `inputs/` and leave the lock file unchanged.
- **REQ-OUTR.SITE.GEN.IMPORT-008** The verify step shall make no network request.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/site/import/ok` (two releases, each with a manifest and four report files, plus an app list, a token file and a spec report, all pinned), when `fid-outreach site import verify --site ok --stage stage --revision abc123 --revision-date 2026-11-17` runs, then it exits 0 and `stage/build-manifest.json` equals `expected/build-manifest.json` byte for byte.
- **AC-2** (digest) Given fixture `tampered` (one byte of `inputs/v0.1.0/parity.json` changed), when verify runs, then it exits 1 with `IMPORT-DIGEST parity.json` and both digests.
- **AC-3** (release cross-check) Given fixture `wrong-release` (a `status.json` whose lock entry matches the file but whose digest differs from the release manifest's entry), when verify runs, then it exits 1 with `IMPORT-RELEASE status.json v0.1.0`.
- **AC-4** (unlisted) Given fixture `stray` (an extra file `inputs/notes.txt`), when verify runs, then it exits 1 with `IMPORT-UNLISTED inputs/notes.txt`.
- **AC-5** (fetch) Given a local test HTTP server on `127.0.0.1` serving fixture `remote/v0.2.0/` and `hosts.toml` listing `127.0.0.1`, when `fid-outreach site import fetch --site work --release v0.2.0 --base-url http://127.0.0.1:<port>/v0.2.0/` runs, then it exits 0, writes five files under `work/inputs/v0.2.0/` and the lock file gains five entries sorted by name.
- **AC-6** (fetch refused) Given `hosts.toml` without `example.org`, when fetch runs with `--base-url https://example.org/v0.2.0/`, then it exits 1 with `IMPORT-HOST example.org` and the lock file is byte-identical to before.
- **AC-7** (no network in verify) Given fixture `ok` run inside a sandbox with networking disabled, then verify exits 0.
- **AC-8** (determinism) Running AC-1 twice, and on Windows, Linux and macOS, gives byte-identical `build-manifest.json`.

## Design

- **Commands:** `fid-outreach site import verify --site <dir> --stage <dir> --revision <sha> --revision-date <date>` (W1: writes only the staging folder) and `fid-outreach site import fetch --site <dir> --release <tag> --base-url <url>` (W1: writes only `inputs/` and the lock file inside the site repository; network read from an allowlisted project host, no data leaves the machine). Project tooling, not registry commands.
- **Lock file:** `[[input]] name, kind, release?, source_rev?, date, path, sha256`; entries sorted by `path`. Spec-tree reports that belong to a release (for example its `COVERAGE.md` and `INDEX.md`) are stored under `inputs/<tag>/` with both `release` and `source_rev`; fetch fills `date` from the manifest's release date. `hosts.toml`: `[[host]] name, purpose (release_host|mirror|site)`; hosts are compared case-insensitively and exactly (no wildcard).
- **Release-manifest contract:** reads only `version` and `artefacts[]` with `name` and `sha256` from `release-manifest.json` schema v1 (ECO.REL.PROC.CHANNEL); other fields are passed through untouched to the staged copy.
- **Algorithm (verify):** parse the lock file (bounded, 256 KiB); hash each file with SHA-256 in streaming mode; list `inputs/` recursively and diff against lock paths; for each release, load its manifest and cross-check its assets; copy inputs into `<stage>/data/<path>`; write `build-manifest.json`.
- **Algorithm (fetch):** check host; download manifest into memory (cap 4 MiB); download each named report (cap 32 MiB each); verify each against the manifest; write all files to a temporary folder; on success move them into `inputs/<tag>/` and rewrite the lock file.
- **Errors:** `IMPORT-DIGEST(name, expected, actual)`, `IMPORT-RELEASE(name, tag)`, `IMPORT-UNLISTED(path)`, `IMPORT-MISSING(name)`, `IMPORT-KIND(kind)`, `IMPORT-HOST(host)`, `IMPORT-FETCH(url, status)`, `INPUT-PARSE` (exit 2).
- **Determinism:** D0 for verify; fetch output depends only on the fetched bytes.
- **Performance budget:** verifying 200 inputs totalling 50 MB in under 2 s on a PLAT reference machine [unverified until measured].
- **Crate:** `fid-outreach`, module `site::import`; HTTP client limited to HTTPS and loopback HTTP for tests.

## Test fixtures

Synthetic fixtures under `fixtures/outr/site/import/` (`ok`, `tampered`, `wrong-release`, `stray`, `remote/v0.2.0/`) with small dummy reports and manifests that validate against `release-manifest.v1.json`; CC-BY-4.0. The fetch test uses a loopback HTTP server started by the test.

## Build prompt

> Add module `site::import` to the `fid-outreach` Rust crate (outreach tooling, not a product registry command) with two subcommands.
>
> `fid-outreach site import verify --site <dir> --stage <dir> --revision <sha> --revision-date <YYYY-MM-DD>`, also exposed as the library function `verify_and_stage`: read `<site>/inputs.lock.toml` (`[[input]]` with name, kind, optional release, optional source_rev (at least one required), date YYYY-MM-DD, path, sha256; kinds release_manifest, release_asset, spec_report, app_list, launch_plan, tokens, brand_asset; reject others). Hash every listed file (SHA-256, streaming); fail `IMPORT-MISSING` or `IMPORT-DIGEST` (print both digests); fail `IMPORT-UNLISTED` for any file under `inputs/` not in the lock; for each `release_asset`, load the same release's `release-manifest.json` (read only `version` and `artefacts[].name`/`sha256`) and fail `IMPORT-RELEASE` when the asset is absent or its digest differs. On success copy inputs to `<stage>/data/` and write `<stage>/build-manifest.json` as RFC 8785 canonical JSON: site revision, revision date, and per input (sorted by path) name, path, kind, release, source_rev, date, sha256 (omit absent optional fields). Never use the network.
>
> `fid-outreach site import fetch --site <dir> --release <tag> --base-url <url>`: refuse (`IMPORT-HOST`) unless the URL host is listed exactly in `<site>/hosts.toml`; download `release-manifest.json` (cap 4 MiB) and, when listed in it, `parity.json`, `parity.md`, `status.json`, `status.md` (cap 32 MiB each); verify each against the manifest; only if all pass, move them into `inputs/<tag>/` and rewrite the lock file sorted by path. On any failure write nothing.
>
> Exit 0 success, 1 check failure, 2 unparsable input. Tests: AC-1..AC-8 of spec node OUTR.SITE.GEN.IMPORT with fixtures under `fixtures/outr/site/import/`, including a loopback HTTP server for fetch and an offline run of verify.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; fail closed; BTreeMap only; dependencies under the project SPDX allowlist (MIT OR Apache-2.0 compatible).

## Open questions

- The fetch command assumes ECO.PARITY attaches `parity.*` and `status.*` to each release and lists them in `release-manifest.json` (ECO.PARITY.GEN REQ-004, ECO.REL.PROC.CHANNEL REQ-002). If they are published elsewhere, add their location to fetch.
- `status.json` comes from ECO.PARITY.STATUS, which starts in P1a; until then releases carry no status report and fetch skips it.
- Signature verification of the release manifest is left to people reviewing the lock-file pull request; adding a DSSE check would need a code edge into TRUST, which tooling may not take (CONSTITUTION §2.1), so it would have to run `fid-verify` as an external program.

## Children

_Leaf: none._
