---
id: KRN.MAN.CARD
title: Algorithm IDs and spec-card registry
level: 3
parent: KRN.MAN
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.MAN-003]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-18
---

# KRN.MAN.CARD: Algorithm IDs and spec-card registry

## Purpose

Every kernel algorithm has an ID `<module>.<feature>.<method>@<semver>` and a spec card (the "computational aim"): what it computes, where it applies, which references define it (by designator, edition and clause only), its determinism class, known failure modes and validation evidence. Cards are what experts review and what `AUTO` and `TRUST` serve. This leaf defines the ID grammar, the card schema, the embedded card registry and the check that no algorithm ships without a card.

## Scope

**In:** the ID grammar and parser; the card schema (TOML); cards embedded at compile time from `cards/*.toml` (no runtime I/O); `CardRegistry` lookup; the bidirectional check between an algorithm table's IDs and the cards; a ban on standards text in cards (length and quotation checks).
**Out:** card content for each algorithm (the algorithm's own leaf); serving cards over MCP (`AUTO`); expert attestation of cards (`TRUST`); card digests in the manifest (`KRN.MAN.MANI`).

## Requirements

- **REQ-KRN.MAN.CARD-001** The ID parser shall accept exactly `<module>.<feature>.<method>@<major>.<minor>.<patch>` with lowercase segments matching `[a-z][a-z0-9_]*` and decimal version numbers without leading zeros that each fit in an unsigned 32-bit integer, and reject anything else with a typed error.
- **REQ-KRN.MAN.CARD-002** A card shall have the fields id, title, method summary, applicability limits, references (each a designator, edition and clause or a bibliographic citation), determinism class, known failure modes and validation evidence, and the parser shall reject a card missing any field.
- **REQ-KRN.MAN.CARD-003** `check(table_ids, registry)` shall return every algorithm ID without a card and every card without an algorithm, and CI shall fail if either list is non-empty.
- **REQ-KRN.MAN.CARD-004** The card checker shall fail on any free-text field longer than 2 000 characters or containing a quoted passage longer than 15 words attributed to a standard, to keep standards text out of cards.
- **REQ-KRN.MAN.CARD-005** The registry shall serve each card's canonical bytes and their SHA-256 digest (via `KRN.MAN.KCI` hashing at the call site) for the manifest and for `AUTO` and `TRUST`.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/card-ids-v1` (50 valid and 50 invalid IDs, such as `fit.cylinder.ls@1.0.0` valid and `Fit.cylinder.ls@1.0`, `fit.cylinder@1.0.0`, `fit.cylinder.ls@01.0.0` invalid), when parsed, then every classification matches.
- **AC-2** Given fixture `fixtures/man/cards-v1` (3 valid cards, 3 cards each missing one field), when parsed, then the valid cards load and each invalid card names its missing field.
- **AC-3** (error path) Given a table with IDs {A, B} and cards {B, C}, when `check` runs, then it reports A as missing a card and C as orphaned, and the CI wrapper exits non-zero.
- **AC-4** Given a card whose references field contains a 40-word quoted passage, then the checker fails with `StandardsTextSuspected`.

## Design

- **Registry command(s):** none.
- **Data structures:** `AlgorithmId { module, feature, method, version: (u32, u32, u32) }` with total ordering; `SpecCard` struct mirroring the schema; `CardRegistry` as a sorted `Vec<(AlgorithmId, &'static str)>` built by a build script that embeds `cards/*.toml` with `include_str!`.
- **Algorithm:** hand-written ID parser; TOML parsing by an allowlisted, pure-Rust TOML crate at build time only (cards are validated in the build script and tests, then embedded as canonical strings).
- **Errors:** `CardError::{BadId, MissingField, StandardsTextSuspected, Orphan, Missing}`.
- **Determinism:** D0.
- **Performance budget:** not critical.
- **Crate:** `fid-kci` (module `cards`) and its build script.

## Test fixtures

`fixtures/man/card-ids-v1` and `fixtures/man/cards-v1`: hand-written, CC-BY-4.0.

## Build prompt

> In crate `fid-kci`, create module `cards`. Implement `AlgorithmId` parsing (`<module>.<feature>.<method>@<major>.<minor>.<patch>`, lowercase `[a-z][a-z0-9_]*` segments, no leading zeros) with total ordering; a `SpecCard` schema (id, title, method summary, applicability limits, references as designator+edition+clause or citations, determinism class D0|D1|D2|D3|N, known failure modes, validation evidence); a build script that reads `cards/*.toml`, validates every card (rejecting missing fields, free-text fields over 2 000 characters and quoted passages over 15 words attributed to standards) and embeds canonical card strings with `include_str!` into a sorted `CardRegistry` (no runtime I/O); and `pub fn check(table_ids: &[AlgorithmId], reg: &CardRegistry) -> CheckReport` listing missing and orphaned cards. Tests: `fixtures/man/card-ids-v1`, `fixtures/man/cards-v1`, the {A,B} vs {B,C} check, standards-text detection. No `unsafe`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic` in non-test code. Acceptance: AC-1..AC-4 of spec node KRN.MAN.CARD.

## Open questions

- None.

## Children

_Leaf: none._
