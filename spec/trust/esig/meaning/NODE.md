---
id: TRUST.ESIG.MEANING
title: Signature meanings catalogue
level: 3
parent: TRUST.ESIG
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.ESIG-008]
covers: []
depends_on: [TRUST.PROFILE.DEFS]
side_effect: R0
determinism: D0
cli: [fid sign list-meanings]
mcp: [sign_list_meanings]
journal: [sign.list_meanings@1]
built_in: [fid-esig]
built_pr: 487
updated: 2026-11-22
---

# TRUST.ESIG.MEANING: Signature meanings catalogue

## Purpose

Defines what a signature can mean, as machine-readable codes with display labels: a fixed built-in set plus tenant-defined meanings supplied by the active governance profile. `fid-verify` reports meanings by code without any localisation data, and the ceremony offers only meanings from this catalogue.

## Scope

**In:**
- Built-in codes `AUTHORED`, `REVIEWED`, `APPROVED`, `RELEASED`
- Tenant meanings read from the active profile document (prefix `X_`)
- Code syntax, default English label, optional localised labels
- `sign.list_meanings@1` (R0)

**Out:**
- Which meanings a profile requires and in what order (`TRUST.PROFILE.DEFS`)
- Recording a meaning in a signature (`TRUST.ESIG.BIND`)

## Requirements

- **REQ-TRUST.ESIG.MEANING-001** The catalogue shall contain the built-in codes `AUTHORED`, `REVIEWED`, `APPROVED` and `RELEASED`, each with a default English label and a one-sentence description written by the project.
- **REQ-TRUST.ESIG.MEANING-002** A meaning code shall match `^[A-Z][A-Z0-9_]{1,31}$`, and every tenant-defined code shall start with `X_`.
- **REQ-TRUST.ESIG.MEANING-003** If the active profile defines a tenant meaning whose code collides with a built-in code, breaks the syntax rule, or appears twice, then loading the catalogue shall fail with `meaning.invalid` naming the code.
- **REQ-TRUST.ESIG.MEANING-004** When `sign.list_meanings@1` runs, it shall return every meaning with code, label, description and origin (`builtin | profile`), ordered built-ins first then tenant codes by code.
- **REQ-TRUST.ESIG.MEANING-005** Built-in codes shall never be renamed or removed once released; a change shall add a new code.

## Acceptance criteria

- **AC-1** Given the built-in Lean profile, when `fid sign list-meanings --json` runs, then exactly the four built-in codes are returned in order.
- **AC-2** Given profile fixture `fixtures/profile/regulated-with-xqa.json` defining `X_QA_RELEASE`, then the list has five entries with the tenant code last and origin `profile`.
- **AC-3** (error path) Given `fixtures/profile/bad-meaning-collision.json` defining `APPROVED` and `fixtures/profile/bad-meaning-syntax.json` defining `x_lower`, then each fails with `meaning.invalid`.
- **AC-4** (MCP) Given AC-1's state, when `sign_list_meanings` is called, then the result equals the CLI JSON.

## Design

- **Registry command(s):** `sign.list_meanings@1`, R0, no params; result `Vec<Meaning>`. Activity Log only.
- **Data structures:** `Meaning { code: MeaningCode, label: String, description: String, origin: Origin, labels: BTreeMap<Lang, String> }`.
- **Algorithm:** static built-in table + validated tenant entries from the profile query API; ordered with `BTreeMap`.
- **Errors:** `meaning.invalid`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-esig` (module `meaning`).

## Test fixtures

- `fixtures/profile/*.json` profile documents (shared with `TRUST.PROFILE.DEFS`), synthetic, CC-BY-4.0.

## Build prompt

> In crate `fid-esig`, add module `meaning` and register `sign.list_meanings@1` (R0; CLI `fid sign list-meanings`; MCP `sign_list_meanings`). Built-in codes `AUTHORED`, `REVIEWED`, `APPROVED`, `RELEASED` with original English labels and descriptions. Tenant meanings come from the active profile document (via the `fid-profile` query API) and must start with `X_`; all codes match `^[A-Z][A-Z0-9_]{1,31}$`; collisions, duplicates or bad syntax fail with `meaning.invalid`. Output ordered built-ins first, then tenant codes sorted. Built-in codes are append-only across releases (add a test that pins the list). Tests: the four acceptance criteria with fixtures in `fixtures/profile/`. Constraints: CONSTITUTION §8, D0, no std HashMap.

## Open questions

- Whether descriptions should cite 21 CFR 11.50 by section number in docs is left to `TRUST.PROFILE.MAP`.

## Children

_Leaf: none._
