---
id: OUTR.BRAND.GUIDE.DRESS
title: Trade-dress distinctness list
level: 4
parent: OUTR.BRAND.GUIDE
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND.GUIDE-004]
covers: []
depends_on: [OUTR.BRAND.GUIDE.NAME]
uses_artifacts: [OUTR.BRAND.LINT]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-outreach]
built_pr: 468
updated: 2026-11-22
---

# OUTR.BRAND.GUIDE.DRESS: Trade-dress distinctness list

## Purpose

For each incumbent named in the seed inventories, a short description in our own words of the distinctive colours, icon styles and layout traits that project material must not imitate. Reviewers use it in the human trade-dress review. Each entry is written from public observation, contains no copied material and no images, and is confirmed by a maintainer before it counts.

## Scope

**In:** `brand/guidelines/trade-dress.toml` (one entry per incumbent) and its rendered page; the completeness and review-state test.
**Out:** the review gate that applies the list (`OUTR.BRAND.REVIEW`); the incumbent-name list (`OUTR.BRAND.LINT`, `rules/incumbents.txt`); visual identity (`OUTR.BRAND.GUIDE.VISUAL`).

## Requirements

- **REQ-OUTR.BRAND.GUIDE.DRESS-001** The list shall have one entry per product in the claim lint's incumbent-name list, each with fields `colours`, `icon_style` and `layout`, written in our own words, and a test shall fail if a product has no entry.
- **REQ-OUTR.BRAND.GUIDE.DRESS-002** Each entry shall carry `reviewed_by` and `reviewed_on` fields, and entries without them shall be rendered as `[unverified]` and excluded from the reviewer-facing summary.
- **REQ-OUTR.BRAND.GUIDE.DRESS-003** Entries shall contain no images, no links to an incumbent's media files, and no text longer than 300 characters per field, and the test shall fail otherwise.
- **REQ-OUTR.BRAND.GUIDE.DRESS-004** The list shall state, per entry, what project material does instead (for example which token roles replace a typical incumbent accent colour).

## Acceptance criteria

- **AC-1** Given `brand/guidelines/trade-dress.toml` and `rules/incumbents.txt`, when `tests/trade_dress.rs` runs, then every incumbent has an entry with `colours`, `icon_style`, `layout` and `instead`.
- **AC-2** (error path) Given fixture `fixtures/outr/brand/dress/missing.toml` (one incumbent without an entry), when the test runs against it, then it fails naming the incumbent.
- **AC-3** (error path) Given `long.toml` (a `layout` field of 400 characters) or `image.toml` (a field containing an image link), when the test runs, then it fails naming the entry and field.
- **AC-4** Given an entry without `reviewed_by`, when the page renderer runs, then the entry is marked `[unverified]` and is absent from the summary.

## Design

- **Registry command(s):** none; a data file, a renderer (`fid-outreach dress-render --list <file> --out <file>`) and a test. Project tooling; nearest class R0.
- **Entry:** `[[product]] name, colours, icon_style, layout, instead, reviewed_by?, reviewed_on?`.
- **Content rule:** describe traits generally (for example "a dominant saturated single accent colour in headers"), never reproduce logos, screenshots, slogans or interface text.
- **Errors:** `entry_missing`, `field_too_long`, `media_reference`, `parse`.
- **Determinism:** D0.
- **Crate:** `fid-outreach`, module `dress`; data in the outreach repository.

## Test fixtures

Hand-written TOML fixtures under `fixtures/outr/brand/dress/` (`missing`, `long`, `image`), CC-BY-4.0, using fictional product names.

## Build prompt

> In the outreach repository, create `brand/guidelines/trade-dress.toml` with one `[[product]]` entry per name in `rules/incumbents.txt`: `name`, `colours`, `icon_style`, `layout`, `instead` (what project material does instead, by design-token role), each at most 300 characters, written in your own words from general public observation, with no images, no links to competitor media and no quoted text. Leave `reviewed_by` and `reviewed_on` empty: a maintainer fills them after review.
>
> In the Rust crate `fid-outreach`, add `fid-outreach dress-render --list <file> --out <file>` that renders the list as Markdown (`{{FAMILY_NAME}}` for the family name), marking unreviewed entries `[unverified]` and leaving them out of the reviewer summary, and `tests/trade_dress.rs` that fails on a missing entry, a field over 300 characters, or any image or media link. Fixtures under `fixtures/outr/brand/dress/`.
>
> Constraints: no panics, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; never fetch or store competitor material.

## Open questions

- Describing incumbents' trade dress precisely enough to avoid it, without copying their material, needs care; each entry stays `[unverified]` until a maintainer reviews it (carried from OUTR.BRAND.GUIDE).

## Children

_Leaf: none._
