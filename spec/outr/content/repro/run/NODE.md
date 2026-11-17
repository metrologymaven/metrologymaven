---
id: OUTR.CONTENT.REPRO.RUN
title: Content replay harness and reproducibility report
level: 4
parent: OUTR.CONTENT.REPRO
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.REPRO-001, REQ-OUTR.CONTENT.REPRO-002, REQ-OUTR.CONTENT.REPRO-005]
covers: []
depends_on: [OUTR.CONTENT.PUB]
uses_artifacts: [CORE.RPL, ECO.REL.PKG, ECO.DATA.EX]
side_effect: W1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.REPRO.RUN: Content replay harness and reproducibility report

## Purpose

When a release candidate is tagged, replays every journal named in a content manifest (tutorials, migration guides, videos) with that candidate's own `fid` binary, reads the Replay Report it writes, and publishes one reproducibility report for the release. It also owns the output normalisation rules that tutorial expected outputs are compared under. It is outreach tooling that runs a released binary as an artifact; it links no product crate.

## Scope

**In:** the `content-replay` subcommand; invoking the candidate's replay command per journal; reading the overall outcome from each Replay Report; the reproducibility report (JSON and Markdown); the normalisation rule file and its `normalise` module.
**Out:** the replay engine and outcome rules (`CORE.RPL`); manifest parsing and validation (`OUTR.CONTENT.PUB`, reused as a module); outdated markers and manifest updates (`OUTR.CONTENT.REPRO.MARK`); example project bundles (`ECO.DATA.EX`); the release binaries (`ECO.REL.PKG`).

## Requirements

- **REQ-OUTR.CONTENT.REPRO.RUN-001** When `content-replay` runs on a content root and a release candidate, it shall replay every journal named in a manifest under that root with the candidate's `fid` binary and record one outcome per content item: `IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`, as read from the Replay Report.
- **REQ-OUTR.CONTENT.REPRO.RUN-002** If the replay process exits abnormally, exceeds its timeout, or writes a Replay Report that does not parse, then the item's outcome shall be `NOT_REPLAYABLE` with the reason recorded.
- **REQ-OUTR.CONTENT.REPRO.RUN-003** The tool shall write one report per release listing, for each item, its content file, kind, journal path, journal SHA-256, outcome, reason when not `IDENTICAL`, and the SHA-256 of the Replay Report, sorted by content file, as RFC 8785 JSON plus a Markdown rendering.
- **REQ-OUTR.CONTENT.REPRO.RUN-004** The tool shall pass the replay process only the candidate binary path, the example project directory and the journal path, and shall itself open no network connection.
- **REQ-OUTR.CONTENT.REPRO.RUN-005** The tool shall ship a versioned normalisation rule file (UUIDs, RFC 3339 timestamps, elapsed-time fields and temporary paths replaced by fixed tokens) and a `normalise` function that other outreach checks reuse.
- **REQ-OUTR.CONTENT.REPRO.RUN-006** The tool shall exit 0 when every item is `IDENTICAL` or `EQUIVALENT`, 1 when any item is `DIVERGENT` or `NOT_REPLAYABLE`, and 2 when the content root, binary or example directory cannot be read.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/outr/repro/ok` (two content items whose journals replay `IDENTICAL` with the stub binary `fixtures/outr/repro/fid-stub`, which prints a fixed Replay Report), when `fid-outreach content-replay --root content --fid fid-stub --examples ex --release v0.3.0 --out out` runs, then it exits 0 and `out/repro-v0.3.0.json` lists both items as `IDENTICAL`.
- **AC-2** Given fixture `fixtures/outr/repro/divergent` (stub reports `DIVERGENT` for one journal), when the tool runs, then it exits 1 and that item is `DIVERGENT`.
- **AC-3** (error path) Given fixture `fixtures/outr/repro/crash` (stub exits 101 without a report), when the tool runs, then the item is `NOT_REPLAYABLE` with reason `replay_failed` and the tool exits 1.
- **AC-4** (timeout) Given a stub that sleeps past `--timeout 2`, when the tool runs, then the item is `NOT_REPLAYABLE` with reason `timeout`.
- **AC-5** (offline) Given AC-1 run in a CI job with networking disabled, then the result equals AC-1.
- **AC-6** (normalisation) Given the strings in `fixtures/outr/repro/normalise.txt`, when `normalise` is applied, then the output equals `normalise.expected.txt` byte-for-byte.
- **AC-7** (determinism) Given AC-1 run twice, then the two JSON reports are byte-identical.

## Design

- **Command:** `fid-outreach content-replay --root <dir> --fid <binary> --examples <dir> --release <tag> --out <dir> [--timeout <s>] [--json]`. Project tooling, not a registry command; nearest class W1 (writes the report into a local output directory).
- **Replay invocation:** a command template in the tool's config, default `{fid} journal replay-cross-version --project {project} --journal {journal} --json` [unverified: the final CLI name is fixed by the CORE.RPL leaves]. The tool reads only the Replay Report's overall `outcome` and, if present, `first_divergence`.
- **Algorithm:** load manifests with the PUB module (failing items reported as `NOT_REPLAYABLE`, reason `manifest_invalid`); for each item in sorted order resolve `{examples}/{example_project}`; spawn the process with a cleared environment except `PATH`, `HOME` and `TMPDIR`; wait with timeout (default 600 s); parse the report with a bounded JSON reader; hash it; collect; write the reports.
- **Normalisation rules** (`normalise.toml`, `version` field): UUID pattern -> `<uuid>`; RFC 3339 date-time -> `<time>`; JSON keys ending `_ms` or `_s` under `timing` -> `<elapsed>`; the temporary directory prefix -> `<tmp>`. Applied in that order.
- **Errors:** fatal `RootUnreadable`, `BinaryMissing`, `ExamplesUnreadable`; item reasons `manifest_invalid`, `example_missing`, `replay_failed`, `timeout`, `report_invalid`.
- **Determinism:** D1. The report is a pure function of the replay outcomes; the outcomes come from the product's replay, which is per-platform bit-exact at best, so CI runs it on each PLAT Tier 1 OS.
- **Performance budget:** dominated by replay; the harness itself adds under 50 ms per item [unverified until measured].
- **Crate:** `fid-outreach`, modules `content_replay` and `normalise`.

## Test fixtures

Synthetic fixtures under `fixtures/outr/repro/`: a stub `fid` script per platform that writes a canned Replay Report, small content roots with manifests, and normalisation input and expected files. CC-BY-4.0.

## Build prompt

> Build the `content-replay` subcommand and the `normalise` module of the Rust outreach tooling crate `fid-outreach` (not product code; it runs a released `fid` binary as an external process and links no product crate). The crate already has a `content_check` module that parses content manifests (`kind`, `example_project`, `journal`, `journal_sha256`, `verified_release`); reuse it.
>
> Signature: `fid-outreach content-replay --root <dir> --fid <binary> --examples <dir> --release <tag> --out <dir> [--timeout <s>] [--json]`. For each manifest under `--root` (sorted), run the configured template (default `{fid} journal replay-cross-version --project {project} --journal {journal} --json`) with a cleared environment (keep `PATH`, `HOME`, `TMPDIR`), a timeout (default 600 s), and parse the Replay Report's `outcome`. Abnormal exit, timeout or unparsable report -> `NOT_REPLAYABLE` with reason `replay_failed`, `timeout` or `report_invalid`. Write `repro-<release>.json` (RFC 8785, items sorted by file: file, kind, journal, journal_sha256, outcome, reason, report_sha256) and `repro-<release>.md`. Exit 0 if all items are `IDENTICAL` or `EQUIVALENT`, 1 otherwise, 2 if the root, binary or examples directory is unreadable. Open no network connection.
>
> `normalise(text) -> String` applies the versioned rules in `normalise.toml`: UUIDs -> `<uuid>`, RFC 3339 date-times -> `<time>`, timing fields -> `<elapsed>`, temporary directory prefix -> `<tmp>`, in that order.
>
> Acceptance tests with fixtures under `fixtures/outr/repro/` (`ok`, `divergent`, `crash`, a sleeping stub for the timeout, `normalise.txt` and its expected output): as listed in the spec; also two runs give byte-identical JSON.
>
> Constraints: no panics and no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads (Replay Reports over 100 MB refused); kill and reap child processes on timeout; BTreeMap only; no network; allowlisted dependency licences.

## Open questions

- Whether `EQUIVALENT` counts as current for tutorials with exact expected outputs is for maintainers (carried from OUTR.CONTENT.REPRO); this leaf treats it as current, and TUT.CHECK compares outputs after normalisation.
- If CORE.RPL fixes exact mode (not cross-version) as the right mode for journals recorded on an earlier release, only the default template changes.

## Children

_Leaf: none._
