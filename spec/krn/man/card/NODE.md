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
built_in: [fid-kci]
built_pr: 45
satisfies: [REQ-KRN.MAN-003]
covers: []
depends_on: []
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-20
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
- **REQ-KRN.MAN.CARD-006** A card shall declare `input_arity`, the number of input blobs the algorithm takes, as `"N"` (exactly N), `"N..=M"` (N to M inclusive, M > N) or `"N.."` (N or more), with decimal numbers without leading zeros and a finite upper bound (N in `"N"`, M in `"N..=M"`) of at most 4 294 967 294 (`MAX_ARITY_BOUND`, so no two spellings accept the same calls on wasm32, where `usize` is 32 bits).
- **REQ-KRN.MAN.CARD-007** If a card has no `input_arity` or spells it any other way, then the parser shall reject the card with `MissingField("input_arity")` or `BadArity`.
- **REQ-KRN.MAN.CARD-008** The canonical card bytes (REQ-005) shall include `input_arity` in its card spelling.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/man/card-ids-v1` (50 valid and 50 invalid IDs, such as `fit.cylinder.ls@1.0.0` valid and `Fit.cylinder.ls@1.0`, `fit.cylinder@1.0.0`, `fit.cylinder.ls@01.0.0` invalid), when parsed, then every classification matches.
- **AC-2** Given fixture `fixtures/man/cards-v1` (3 valid cards, 3 cards each missing one field), when parsed, then the valid cards load and each invalid card names its missing field.
- **AC-3** (error path) Given a table with IDs {A, B} and cards {B, C}, when `check` runs, then it reports A as missing a card and C as orphaned, and the CI wrapper exits non-zero.
- **AC-4** Given a card whose references field contains a 40-word quoted passage, then the checker fails with `StandardsTextSuspected`.
- **AC-5** Given the spellings `0`, `1..=3`, `2..`, `4294967294` and `4294967295..`, then each parses and prints back unchanged; given `01`, `1..=1`, `3..=2`, `..3`, `1..2`, `4294967295`, `0..=4294967295` or `4294967296`, then the parser fails with `BadArity`; a card without `input_arity` fails with `MissingField("input_arity")`; `fixtures/man/card-digests-v1.txt` shows the field in the canonical bytes.

## Design

- **Registry command(s):** none.
- **Data structures:** `AlgorithmId { module, feature, method, version: (u32, u32, u32) }` with total ordering; `SpecCard` struct mirroring the schema; `CardRegistry` as a sorted `Vec<(AlgorithmId, &'static str)>` built by a build script that embeds `cards/*.toml` with `include_str!`.
- **Algorithm:** hand-written ID parser; TOML parsing by an allowlisted, pure-Rust TOML crate at build time only (cards are validated in the build script and tests, then embedded as canonical strings).
- **Errors:** `CardError::{BadId, MissingField, StandardsTextSuspected, Orphan, Missing, BadArity}`.
- **Determinism:** D0.
- **Performance budget:** not critical.
- **Crate:** `fid-kci` (module `cards`) and its build script.

## Test fixtures

`fixtures/man/card-ids-v1` and `fixtures/man/cards-v1`: hand-written, CC-BY-4.0.

## Build prompt

> In crate `fid-kci`, create module `cards`. Implement `AlgorithmId` parsing (`<module>.<feature>.<method>@<major>.<minor>.<patch>`, lowercase `[a-z][a-z0-9_]*` segments, no leading zeros) with total ordering; a `SpecCard` schema (id, title, method summary, applicability limits, references as designator+edition+clause or citations, determinism class D0|D1|D2|D3|N, known failure modes, validation evidence); a build script that reads `cards/*.toml`, validates every card (rejecting missing fields, free-text fields over 2 000 characters and quoted passages over 15 words attributed to standards) and embeds canonical card strings with `include_str!` into a sorted `CardRegistry` (no runtime I/O); and `pub fn check(table_ids: &[AlgorithmId], reg: &CardRegistry) -> CheckReport` listing missing and orphaned cards. Tests: `fixtures/man/card-ids-v1`, `fixtures/man/cards-v1`, the {A,B} vs {B,C} check, standards-text detection. No `unsafe`, no HashMap/HashSet; deny `unwrap`/`expect`/`panic` in non-test code. Acceptance: AC-1..AC-4 of spec node KRN.MAN.CARD.

## Open questions

- None.

## Schema changes

- **2026-11-20: `input_arity` added (REQ-006 .. -008).** Compatibility rule: [D-046](../../../../decisions/D-046-card-input-arity-compatibility.md). Required, so a card written before this change is refused (`MissingField`) until it declares its inputs; fail closed, because `KRN.MAN.KCI` REQ-008 refuses calls against this field. The field is part of the canonical card bytes, so every card digest changed: `fixtures/man/card-digests-v1.txt` was regenerated by `make_kci_fixtures.py`, and the four `fid-fit` cards (`fit.{point,line2,line3,plane}.ls@1.0.0`, each `"1"`) were updated. Algorithm IDs were not bumped (D-046 rule 1): outputs for every accepted call stay bit-identical and no release manifest or card digest has been published. From now on, any change to a released algorithm's `input_arity`, or anything else that narrows or widens the calls it accepts, requires an algorithm semver bump (D-046 rule 2). A change to a card's schema or canonical bytes is listed in this section with its date and the cards it touched.

## Children

_Leaf: none._
