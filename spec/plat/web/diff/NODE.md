---
id: PLAT.WEB.DIFF
title: Generated desktop-versus-web feature difference list
level: 3
parent: PLAT.WEB
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-PLAT.WEB-004]
covers: []
depends_on: [CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# PLAT.WEB.DIFF: Generated feature difference list

## Purpose

Generates the published list of what the web build can and cannot do compared with desktop, directly from the registry manifest, and fails CI when any command has not declared its web availability. Because the list is generated, it can never drift from the code (CONSTITUTION §8, honest status).

## Scope

**In:**
- Reading the canonical registry manifest (JSON)
- Checking that every command declares `web` availability (`available`, `partial` or `unavailable`) and, for `partial` and `unavailable`, a short note
- Generating `docs/platform/web-differences.md` and `platform/web-differences.json`
- `--check` mode for CI

**Out** (and which node owns it instead):
- Adding the `web` and `web_note` fields to the manifest schema and to each command declaration (`CORE.REG`, then each command's owner)
- Publishing the docs site (`OUTR.DOCS`)

## Requirements

- **REQ-PLAT.WEB.DIFF-001** When run, the generator shall read the registry manifest and list every command whose `web` value is `partial` or `unavailable`, grouped by domain, with its CLI name, side-effect class and note.
- **REQ-PLAT.WEB.DIFF-002** If any command in the manifest lacks a `web` value, or has `partial` or `unavailable` without a non-empty `web_note`, then the generator shall exit 1 and list each such command.
- **REQ-PLAT.WEB.DIFF-003** The generator shall state the manifest's SHA-256 digest in both outputs.
- **REQ-PLAT.WEB.DIFF-004** When run with `--check`, the generator shall exit 1 with a diff if the committed outputs differ from what it would generate, without writing.
- **REQ-PLAT.WEB.DIFF-005** For the same manifest, the generator shall produce byte-identical outputs on every platform.
- **REQ-PLAT.WEB.DIFF-006** If the manifest exceeds 64 MiB or is not valid JSON, then the generator shall fail closed with a typed error.

## Acceptance criteria

- **AC-1** Given `fixtures/plat/web/manifest-small.json` (5 commands: 3 `available`, 1 `partial`, 1 `unavailable`), when `cargo xtask plat-web diff --out-dir <tmp>` runs, then `web-differences.md` and `web-differences.json` equal `fixtures/plat/web/expected-differences.md` and `.json` byte for byte, and list exactly 2 commands.
- **AC-2** (error path) Given `fixtures/plat/web/manifest-missing-flag.json` where one command lacks `web`, when run, then it exits 1 naming that command ID and writes nothing.
- **AC-3** (error path) Given a manifest where an `unavailable` command has an empty `web_note`, when run, then it exits 1 naming the command.
- **AC-4** Given the expected outputs with one line edited, when `--check` runs, then it exits 1 and prints a diff; with unedited outputs it exits 0.
- **AC-5** Given the real registry manifest in CI, when `--check` runs, then it exits 0.

## Design

- **Registry command(s):** none; `xtask plat-web diff [--manifest <file>] --out-dir <dir> [--check]`.
- **Data structures:** reads the manifest as generic JSON (only `commands[].id`, `cli`, `side_effect`, `web`, `web_note`, `domain`) so it does not depend on CORE.REG's Rust types; `DiffEntry { domain, id, cli, side_effect, web, note }`.
- **Algorithm:** size check; parse; collect entries; validate flags; sort by (domain, id); render Markdown (header with manifest SHA-256 and "generated, do not edit") and JSON; LF endings.
- **Errors:** `TooLarge`, `Parse`, `MissingWebFlag { ids }`, `MissingWebNote { ids }`, `Mismatch` (check mode).
- **Determinism:** D0.
- **Performance budget:** < 1 s for a 10 000-command manifest [unverified].
- **Crate:** `fid-platform` (module `web_diff`) with the `xtask` subcommand.

## Test fixtures

Hand-written manifest excerpts under `fixtures/plat/web/` with invented command IDs; expected outputs as golden files. Licensed with the code.

## Build prompt

> In crate `fid-platform`, add module `web_diff` and `xtask plat-web diff [--manifest <file>] --out-dir <dir> [--check]`. Read the registry manifest JSON (cap 64 MiB) generically; for each entry in `commands` take `id`, `domain`, `cli`, `side_effect`, `web` (`available` | `partial` | `unavailable`) and `web_note`. Fail (exit 1, write nothing) if any command lacks `web` or has `partial`/`unavailable` without a non-empty note, listing all offenders. Otherwise write `web-differences.md` (header with manifest SHA-256 and a do-not-edit line; one section per domain; table of CLI name, class, availability, note) and `web-differences.json`, sorted by (domain, id), LF endings. `--check` compares instead of writing and prints a diff.
>
> Acceptance tests: the five criteria above with fixtures in `fixtures/plat/web/`. Constraints: CONSTITUTION §8; outputs byte-identical across platforms. The `web` and `web_note` manifest fields are added by CORE.REG; until then, AC-5 is skipped with a tracked issue. Commit: `feat(fid-platform): generated web feature difference list`.

## Open questions

- CORE.REG's manifest schema does not yet carry a `web` availability flag; this leaf needs it (cross-domain request to CORE.REG).

## Children

_Leaf: none._
