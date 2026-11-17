---
id: OUTR.CONTENT.TUT.CORE
title: First steps: CLI, journal, replay and verification
level: 4
parent: OUTR.CONTENT.TUT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT.TUT-002, REQ-OUTR.CONTENT.TUT-003, REQ-OUTR.CONTENT.TUT-004, REQ-OUTR.CONTENT.TUT-005]
covers: []
depends_on: [OUTR.CONTENT.TUT.CHECK, OUTR.CONTENT.PUB]
uses_artifacts: [ECO.DATA.EX, ECO.REL.PKG, TRUST.VERIFY.CLI]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.CONTENT.TUT.CORE: First steps: CLI, journal, replay and verification

## Purpose

The first tutorial every new user and contributor can follow without any app: run a command from the `fid` CLI on a small example project, read the journal entry it wrote, replay the journal, and verify a result package with `fid-verify`. It shows the project's core promise (everything recorded, everything replayable, verification free) in about 20 minutes.

## Scope

**In:** the tutorial text; its example project (a small point set and a fit, for example a least-squares circle) and journal; expected outputs for every step.
**Out:** app tutorials (`OUTR.CONTENT.TUT.REPORT`, `OUTR.CONTENT.TUT.INSPECT`); the user-guide chapters on journals and verification (`OUTR.DOCS.BOOK.CONCEPT`); the check tooling (`OUTR.CONTENT.TUT.CHECK`).

## Requirements

- **REQ-OUTR.CONTENT.TUT.CORE-001** The tutorial shall declare `app: core` and `topics` containing `cli`, `journal`, `replay` and `verify`.
- **REQ-OUTR.CONTENT.TUT.CORE-002** The tutorial shall have one section each for running a command with `--json`, reading the resulting journal entry, replaying the journal in exact mode, and verifying a result package with `fid-verify`.
- **REQ-OUTR.CONTENT.TUT.CORE-003** Every command step shall be a `sh step` block followed by a `text expected` block, and `tut-check --run` shall pass on the targeted release.
- **REQ-OUTR.CONTENT.TUT.CORE-004** Where the targeted release's verifier reports a section as `not_available` (D-025, P0 chain-only verifier), the tutorial shall say so plainly instead of hiding it.
- **REQ-OUTR.CONTENT.TUT.CORE-005** The tutorial shall state `duration_min` and its prerequisites, and shall need only the minimum hardware profile.

## Acceptance criteria

- **AC-1** Given the tutorial and its example project, when `fid-outreach tut-check --root content --apps apps.toml --app-status apps-status.json --fid <release fid> --examples ex --run` runs, then it reports no finding for this tutorial.
- **AC-2** Given the tutorial, when `fid-outreach content-check` runs on it, then it exits 0.
- **AC-3** Given the tutorial's journal, when `content-replay` runs on the targeted release, then its outcome is `IDENTICAL`.
- **AC-4** Given the tutorial, when a test lists its headings, then it finds the four sections of REQ-002 in that order.

## Design

- **Registry command(s):** none; a tutorial checked by `content-check`, `tut-check` and `content-replay`.
- **Files:** `content/tutorials/core/tutorial.md` with the OUTR.CONTENT.PUB manifest plus `app: core`, `topics`, `duration_min` and `prerequisites` (release, example project, hardware profile `minimum` per D-014).
- **Command names:** the steps use whatever the targeted release's registry manifest names for a fit, `journal` show and replay commands, and the `fid-verify` binary; the author takes names from the release's reference pages, never from this spec.
- **Determinism:** D1: expected outputs come from the release binary on one platform and are compared after normalisation.
- **Crate:** none (content); checks run from `fid-outreach`.

## Test fixtures

The tutorial's example project and journal, contributed through ECO's example-project process (ECO.DATA.EX) under CC-BY-4.0 or the source dataset's licence; the release binaries of the targeted release.

## Build prompt

> Write `content/tutorials/core/tutorial.md`, a Markdown tutorial for new users of the `fid` CLI. Front matter: the content manifest (`kind: tutorial`, `example_project`, `journal`, `journal_sha256`, `verified_release`) plus `app: core`, `topics: [cli, journal, replay, verify]`, `duration_min`, and `prerequisites` (release, example project, hardware profile `minimum`). Sections in order: run a command with `--json` on the example point set; read the journal entry it wrote; replay the journal in exact mode; verify the result package with `fid-verify`. Each command is a fenced `sh step` block holding one `fid` command line, followed by a `text expected` block with the output it printed on the targeted release. Take command names from the release's reference pages. If the verifier reports signature or replay sections as `not_available`, say so plainly. Done when `fid-outreach content-check` and `fid-outreach tut-check --run` exit 0 on the tutorial, and its journal replays `IDENTICAL` on the targeted release. Write in your own words; use only screenshots made with our `capture.*` commands; no "certified", "compliant" or comparative claims.

## Open questions

- CLI and journal walkthroughs need no app and could start in P0; moving this leaf to P0 needs a phase decision (carried from OUTR.CONTENT.TUT).

## Children

_Leaf: none._
