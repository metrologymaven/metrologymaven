---
id: ECO.DATA.EX
title: Example projects with replayable journals
level: 3
parent: ECO.DATA
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-ECO.DATA-006]
covers: []
depends_on: [ECO.DATA.FETCH]
uses_artifacts: [ECO.DATA.CAT, ECO.APPS.LIST, CORE.RPL.EXEC, CORE.RPL.RETGT, CORE.PRJ.CONT]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.DATA.EX: Example projects with replayable journals

## Purpose

A versioned set of example projects (one or more per app workflow) whose recorded journals replay on each release with outcome `IDENTICAL` or `EQUIVALENT`. They are the shared fixtures for the per-app end-to-end tests, the round-trip suite, the docs and tutorials, and new contributors who want to reproduce a result. This leaf owns the example index, its static check, the release replay check and the rule for changing an expected outcome; the content of each example belongs to the app and domain that need it.

## Scope

**In:**
- Example index `examples/index.toml`: ID, app, workflow, owning domain, datasets used (by catalogue ID and digest), replay mode, expected replay outcome
- Storage rules: project files and journals in `examples/<id>/`, datasets referenced by catalogue ID and fetched, never embedded when `fetch-only`
- `cargo xtask examples check` (static rules and the expected-outcome change rule)
- `cargo xtask examples replay` (release check: replay every example with the `fid` binary and record outcomes)

**Out** (and which node owns it instead):
- Replay engine and outcome classes (owned by `CORE.RPL`; this tool runs `fid journal replay-exact` and `replay-dry-run`)
- Workflow content and expected results for each app (owned by `ECO.APPS.REPORT`, `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK` and the domains)
- Tutorials built on the examples (owned by `OUTR`)

## Requirements

- **REQ-ECO.DATA.EX-001** Each index entry shall have `id` (`^ex-[a-z0-9-]{3,40}$`, unique), `app` (an app ID from `apps.toml`), `workflow` (one line), `owner` (a level-1 domain ID), `datasets` (list of `{id, sha256}` matching the catalogue), `mode` (`exact` or `dry-run`) and `expected` (`IDENTICAL` or `EQUIVALENT`), and the check shall fail with `EX-SCHEMA` otherwise.
- **REQ-ECO.DATA.EX-002** If any file under `examples/<id>/` has the SHA-256 of a catalogue dataset marked `fetch-only`, or any file is larger than 1 MiB and not listed in the entry's `large_files` allowance, then the check shall fail with `EX-EMBEDDED`.
- **REQ-ECO.DATA.EX-003** When `examples replay` runs, it shall replay each example's journal in its mode with the supplied `fid` binary after fetching its datasets, and shall exit 1 if any outcome is `DIVERGENT` or `NOT_REPLAYABLE` or differs from `expected`.
- **REQ-ECO.DATA.EX-004** If an entry's `expected` value or journal file differs from the base revision's index, then the check run with `--base` shall fail with `EX-CHANGE-CAUSE` unless the entry's `cause` field is non-empty and differs from the base value.
- **REQ-ECO.DATA.EX-005** If an app in `apps.toml` has a phase in the `--started` list and no index entry names it, then the check shall fail with `EX-MISSING-APP` naming the app.
- **REQ-ECO.DATA.EX-006** Entries with `mode = "dry-run"` shall carry a `dry_run_reason`, and the replay report shall show their mode, so examples that need device transcripts (Probe, Network) are visibly not exact until HAL simulator replay exists.
- **REQ-ECO.DATA.EX-007** The replay command shall write `examples-replay.json` with `{id, mode, expected, outcome, fid_version}` per example, sorted by ID, for attachment to the release.

## Acceptance criteria

- **AC-1** Given `fixtures/examples/ok/` (index with two entries, one exact, one dry-run with reason; catalogue `datasets.toml`; `apps.toml` with apps `report` (P1a) and `probe` (P2)), when `cargo xtask examples check --index fixtures/examples/ok/index.toml --catalogue ... --apps ... --started P0,P1a` runs, then it exits 0.
- **AC-2** (error path) Given `fixtures/examples/embedded/` where `examples/ex-scan-a/scan.ply` has the digest of a `fetch-only` dataset, then `EX-EMBEDDED ex-scan-a scan.ply`.
- **AC-3** (error path) Given `--started P0,P1a,P1b` and apps `inspect` (P1b) with no example, then `EX-MISSING-APP inspect`.
- **AC-4** (change rule) Given base index `fixtures/examples/base.toml` and head `head-no-cause.toml` changing `expected` from `IDENTICAL` to `EQUIVALENT` with no `cause`, then `EX-CHANGE-CAUSE`; with `head-cause.toml` stating a cause, exit 0.
- **AC-5** (replay) Given the fake `fid` `fixtures/examples/fake-fid` that returns `IDENTICAL` for `ex-a` and `DIVERGENT` for `ex-b`, when `cargo xtask examples replay --index ... --fid fixtures/examples/fake-fid --out <tmp>` runs, then it exits 1, and `examples-replay.json` lists both with their outcomes.
- **AC-6** (determinism) Running AC-1 and AC-5 twice gives byte-identical output.

## Design

- **Registry command(s):** none; repository tool `cargo xtask examples check|replay`. `side_effect: W1`: replay works on copies in a local scratch root and writes the report there.
- **Data structures:** index `[[example]] id, app, workflow, owner, datasets = [{id, sha256}], mode, expected, cause?, dry_run_reason?, large_files = []`; report as REQ-007.
- **Algorithm:** check: parse index, catalogue and app list; hash every example file (bounded walk, 2 GiB total cap) and compare with fetch-only digests; diff against `--base`. Replay: for each example, fetch datasets with `fid-data` into the cache, copy the project to scratch, run `<fid> journal replay-exact --json` or `replay-dry-run --json` on the copy, read the outcome field, compare.
- **Errors:** `EX-SCHEMA`, `EX-EMBEDDED`, `EX-CHANGE-CAUSE`, `EX-MISSING-APP`, `EX-REPLAY` (outcome failure), `EX-INPUT`, `EX-IO`.
- **Determinism:** D0 for the check and the report given the replay outcomes.
- **Performance budget:** replay of all examples within the release pipeline's budget; target under 15 minutes on the CI runner for P1a examples [unverified until measured].
- **Crate:** `fid-xtask` (module `examples`), using the `fid-data` fetch library; runs the `fid` binary, links no product crate.

## Test fixtures

Miniature index, catalogue, app list, example folders and a fake `fid` script under `fixtures/examples/` (MIT OR Apache-2.0). Real examples are added by the per-app leaves (`ex-report-transcription`, `ex-inspect-scan-to-cad`, `ex-probe-prismatic-sim`, `ex-network-recorded`).

## Build prompt

> In `fid-xtask` (implementation repository), add `examples check --index examples/index.toml --catalogue datasets.toml --apps apps.toml --started <phases> [--base <index>]` and `examples replay --index ... --fid <binary> --out <dir>`. Index `[[example]]`: `id` (`^ex-[a-z0-9-]{3,40}$`), `app` (in `apps.toml`), `workflow`, `owner` (level-1 domain), `datasets [{id, sha256}]` (must match catalogue), `mode` (`exact|dry-run`; dry-run needs `dry_run_reason`), `expected` (`IDENTICAL|EQUIVALENT`), optional `cause`, `large_files`. Check findings: `EX-SCHEMA`; `EX-EMBEDDED` (a file under `examples/<id>/` hashes to a `fetch-only` dataset, or exceeds 1 MiB without a `large_files` allowance); `EX-MISSING-APP` (an app whose phase is in `--started` has no example); `EX-CHANGE-CAUSE` (with `--base`: `expected` or the journal changed without a new non-empty `cause`). Replay: fetch datasets via `fid-data`, copy each project to scratch, run `<fid> journal replay-exact --json` or `replay-dry-run --json`, write `examples-replay.json` `[{id, mode, expected, outcome, fid_version}]` sorted, exit 1 if any outcome is `DIVERGENT`, `NOT_REPLAYABLE` or not the expected one. Tooling: run binaries, do not link product crates. Tests for AC-1..AC-6 of spec node ECO.DATA.EX with a fake `fid`. No `unwrap`/`expect`/`panic`/`unsafe`; bounded walks.

## Open questions

- Replay of examples that include device transcripts (Probe, Network) depends on HAL simulator replay; until then those examples run in dry-run mode (REQ-006).
- The replay result's JSON field holding the outcome is owned by CORE.RPL.EXEC and CORE.RPL.RETGT (open); the tool reads one field and adapts when they fix it.

## Children

_Leaf: none._
