---
id: OUTR.BRAND.GUIDE.NAME
title: Name, logo and third-party usage rules
level: 4
parent: OUTR.BRAND.GUIDE
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
built_in: [fid-outreach]
built_pr: 112
satisfies: [REQ-OUTR.BRAND.GUIDE-001, REQ-OUTR.BRAND.GUIDE-002, REQ-OUTR.BRAND.GUIDE-003]
covers: []
depends_on: []
uses_artifacts: []
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.GUIDE.NAME: Name, logo and third-party usage rules

## Purpose

The written rules for how the project family name, the app names (family name plus Inspect, Probe, Network or Report) and the logo may be used by the project and by third parties, drafted now in name-agnostic form with one placeholder token, plus the guard that keeps every guideline file unpublished until the name chosen under D-002 is accepted.

## Scope

**In:** `brand/guidelines/name-and-logo.md`; the placeholder token and the name state file `brand/name.toml`; the `guide-check` command that checks every file under `brand/guidelines/` for the token rule and refuses publication before acceptance.
**Out:** visual identity and screenshot rules (`OUTR.BRAND.GUIDE.VISUAL`); the trade-dress distinctness list (`OUTR.BRAND.GUIDE.DRESS`); choosing the name and trademark matters (D-002, D-001, counsel); logo files (`OUTR.BRAND.ASSET.PACK`).

## Requirements

- **REQ-OUTR.BRAND.GUIDE.NAME-001** The name-and-logo guidelines shall state how third parties may refer to the project and its apps (nominative reference, "works with" wording for compatible products), how they may use the logo, and list uses that imply endorsement and need written permission.
- **REQ-OUTR.BRAND.GUIDE.NAME-002** The guidelines shall give logo rules for clear space, minimum size and forbidden alterations, expressed relative to the logo's height so they hold for any final logo.
- **REQ-OUTR.BRAND.GUIDE.NAME-003** Every file under `brand/guidelines/` shall write the family name only as the token `{{FAMILY_NAME}}`, and `guide-check` shall fail with `literal_name` naming file and line if the working name or any name listed in `brand/name.toml` appears literally.
- **REQ-OUTR.BRAND.GUIDE.NAME-004** When `guide-check --publish` runs and `brand/name.toml` does not record `accepted = true` with the chosen name and a decision reference, then it shall fail with `name_not_accepted` and write nothing.
- **REQ-OUTR.BRAND.GUIDE.NAME-005** When `guide-check --publish` runs with an accepted name, it shall write publishable copies with every token replaced by the chosen name, and fail with `token_left` if any `{{...}}` token remains.

## Acceptance criteria

- **AC-1** Given the guidelines folder and `brand/name.toml` with `accepted = false`, when `fid-outreach guide-check --root brand/guidelines --name brand/name.toml --json` runs, then it exits 0.
- **AC-2** (error path) Given fixture `fixtures/outr/brand/guide/literal/` (a guideline line containing the working name), when the check runs, then it exits 1 with `literal_name` at that file and line.
- **AC-3** (error path) Given `accepted = false`, when `guide-check --publish --out out/` runs, then it exits 1 with `name_not_accepted` and `out/` is empty.
- **AC-4** Given fixture `accepted/` (`accepted = true`, name "Examplo", decision "D-002"), when `guide-check --publish --out out/` runs, then it exits 0 and the copies contain "Examplo Inspect" where the source had `{{FAMILY_NAME}} Inspect`.
- **AC-5** (error path) Given `stray-token/` (an unknown token `{{TAGLINE}}`), when the publish runs with an accepted name, then it exits 1 with `token_left`.

## Design

- **Command:** `fid-outreach guide-check --root <dir> --name <file> [--publish --out <dir>] [--json]`. Project tooling; nearest class R0. Setting `accepted = true` is a human act by pull request after D-002 names the family.
- **Name state file:** `accepted = false`, `working_names = ["Fiducial", "fid"]`, after acceptance `name = "..."`, `decision = "D-002"`, `date`.
- **Literal-name scan:** case-sensitive whole-word match of `working_names` and `name` outside code spans; the command name `fid` in code spans is allowed.
- **Guideline content (in our own words):** referring to the project; app names; "works with" and "built on" wording; endorsement; logo clear space (proposal: one quarter of the logo height), minimum size, forbidden alterations (recolouring outside the palette, stretching, effects, combining with other marks).
- **Errors:** `literal_name`, `name_not_accepted`, `token_left`, `name_file_parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `guide_check`; documents in the outreach repository.

## Test fixtures

Small Markdown folders and name files under `fixtures/outr/brand/guide/` (`literal`, `accepted`, `stray-token`), CC-BY-4.0; "Examplo" is a made-up name.

## Build prompt

> In the outreach repository, write `brand/guidelines/name-and-logo.md` in your own words, using the token `{{FAMILY_NAME}}` wherever the family name appears (apps are `{{FAMILY_NAME}} Inspect`, `Probe`, `Network`, `Report`). Cover: how third parties may refer to the project and apps (nominative use; "works with" for compatible products), uses that imply endorsement and need written permission, and logo rules (clear space and minimum size relative to logo height, forbidden alterations). Add `brand/name.toml` with `accepted = false` and `working_names = ["Fiducial", "fid"]`.
>
> In the Rust crate `fid-outreach`, add `fid-outreach guide-check --root <dir> --name <file> [--publish --out <dir>] [--json]`: report `literal_name` (file, line) for any whole-word working or chosen name outside code spans in Markdown files under `--root`; with `--publish`, fail with `name_not_accepted` unless `accepted = true` with `name` and `decision`, otherwise write copies to `--out` with `{{FAMILY_NAME}}` replaced and fail with `token_left` if any `{{...}}` remains. Exit 0, 1 findings, 2 unreadable input; write nothing on failure.
>
> Tests with fixtures under `fixtures/outr/brand/guide/`: unaccepted check passes, literal name found, publish refused before acceptance, publish after acceptance substitutes, stray token refused.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; nothing copied from any other organisation's brand guidelines.

## Open questions

- Trademark wording (registration marks, licence of the mark) waits for counsel under D-001 and D-002 and is not drafted here.

## Children

_Leaf: none._
