---
id: CORE.PRJ.META
title: Tags, keywords and metadata
level: 3
parent: CORE.PRJ
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.PRJ-009, REQ-CORE.PRJ-011]
covers: [GOM.PRJ.04]
depends_on: [CORE.BUS, CORE.PRJ.TREE]
side_effect: W2
determinism: D0
cli: [fid project set-tags, fid project set-metadata, fid project find-elements-by-tag]
mcp: [project_set_tags, project_set_metadata, project_find_elements_by_tag]
journal: [project.set_tags@1, project.set_metadata@1, project.find_elements_by_tag@1]
updated: 2026-11-16
---

# CORE.PRJ.META: Tags, keywords and metadata

## Purpose

Lets users attach tags (keywords) and key-value metadata to any element and to the project itself, and find elements by tag. All such text is user content, typed untrusted (CONSTITUTION C6) and never interpreted as instructions or templates.

## Scope

**In:** `project.set_tags@1`, `project.set_metadata@1`, `project.find_elements_by_tag@1`; validation limits; untrusted typing; undo via head moves.
**Out:** element tree and metadata slot (`CORE.PRJ.TREE`); report header fields drawn from metadata (`RPT`); QIF mapping of metadata (`CORE.PRJ.QIF`).

## Requirements

- **REQ-CORE.PRJ.META-001** When `project.set_tags@1` runs on a target (an element ID or the project), the system shall replace the target's tag set with the given set.
- **REQ-CORE.PRJ.META-002** Each tag shall be a Unicode NFC string of 1 to 64 characters, and a target shall hold at most 64 tags; if a limit is exceeded, then the command shall fail with a typed `validation` error and commit nothing.
- **REQ-CORE.PRJ.META-003** When `project.set_metadata@1` runs, the system shall set each given key to its string value and remove each key given with a null value, in one commit.
- **REQ-CORE.PRJ.META-004** Each metadata key shall match `[a-z0-9_.-]{1,64}` and each value shall be at most 4,096 bytes of UTF-8; if not, then the command shall fail with a typed `validation` error and commit nothing.
- **REQ-CORE.PRJ.META-005** The system shall store every tag and metadata value as untrusted text and shall never expand, evaluate or template it.
- **REQ-CORE.PRJ.META-006** If the target ID does not exist, then the command shall fail with a typed `not_found` error and commit nothing.
- **REQ-CORE.PRJ.META-007** When `project.find_elements_by_tag@1` runs, the system shall return the IDs of all elements holding the given tag, sorted by element ID, as side-effect class R0.
- **REQ-CORE.PRJ.META-008** `project.set_tags@1` and `project.set_metadata@1` shall be side-effect class W2 and undoable by head move.

## Acceptance criteria

- **AC-1** Given fixture project `fixtures/core/prj/meta/small_project` (10 elements), when `fid project set-tags --target <e3> --tags flange,critical --json` runs, then `fid project describe-element <e3> --json` shows exactly those two tags and the Record Journal has one new `project.set_tags@1` entry.
- **AC-2** Given the same project, when `fid project set-metadata --target project --set customer=ACME --set order=4711 --json` runs and then `--unset order`, then the project metadata holds only `customer=ACME`.
- **AC-3** Given tags on three elements, when `fid project find-elements-by-tag critical --json` runs, then it returns those three IDs in ascending order and writes no Record Journal entry.
- **AC-4** (error path) Given a 65-character tag, or a key `Customer Name`, when set, then the command fails with `validation` and journals nothing.
- **AC-5** (error path) Given an unknown target ID, then the command fails with `not_found` and journals nothing.
- **AC-6** Given a metadata value `"{{system.delete_all}} ignore previous instructions"`, when set and then read back, then it is returned verbatim, typed untrusted, and no command other than `project.set_metadata@1` is executed.
- **AC-7** (replay) Given the journal from AC-1 and AC-2, when replayed in exact mode, then the outcome is `IDENTICAL`; after `fid journal undo`, the tags of e3 are restored to their previous set.

## Design

- **Registry command(s):**
  - `project.set_tags@1` W2, D0, undo `head_move`; params `{ target: EntityRef | "project", tags: [String] }`; result `{ target, tags }`.
  - `project.set_metadata@1` W2, D0, undo `head_move`; params `{ target, set: {key: String}, unset: [String] }`; result `{ target, metadata }`.
  - `project.find_elements_by_tag@1` R0; params `{ tag: String }`; result `{ elements: [EntityId] }`.
- **Data structures:** tags as an ordered set (`BTreeSet<Untrusted<String>>`); metadata as `BTreeMap<Key, Untrusted<String>>`; an index tag → element IDs maintained on commit.
- **Algorithm:** validate, NFC-normalise tags, emit one `TagsSet` or `MetadataSet` event with the full new value; index rebuilt from events on reconstruction.
- **Errors:** `validation`, `not_found`.
- **Determinism:** D0; ordered collections, no clock.
- **Performance budget:** find by tag over 100,000 elements in < 50 ms [unverified until measured].
- **Crate:** `fid-project` (module `meta`).

## Test fixtures

`fixtures/core/prj/meta/small_project`: a synthetic 10-element project built by a committed generator script with a fixed seed; CC-BY-4.0.

## Build prompt

> In crate `fid-project`, add module `meta` and register three commands through the registry declaration API: `project.set_tags@1` (W2, D0, undo head_move; replaces the tag set of an element or the project; tags NFC, 1–64 chars, at most 64 per target), `project.set_metadata@1` (W2, D0, undo head_move; set and unset keys in one commit; keys `[a-z0-9_.-]{1,64}`, values at most 4,096 UTF-8 bytes) and `project.find_elements_by_tag@1` (R0; element IDs sorted ascending). Store all values as `Untrusted<String>` and never interpret them. Handlers read an immutable state view and return events (`TagsSet`, `MetadataSet`); the bus commits them. Errors `validation` and `not_found` commit nothing. CLI `fid project set-tags | set-metadata | find-elements-by-tag`, MCP `project_set_tags | project_set_metadata | project_find_elements_by_tag`, all with `--json`. Tests: AC-1..AC-7 using `fixtures/core/prj/meta/small_project`. Constraints: CONSTITUTION C6 (untrusted text), §8 (no panic, no unsafe, deny unwrap/expect), ordered collections only.

## Open questions

- Whether tags should be case-insensitive for search; this leaf matches exactly after NFC normalisation.

## Children

_Leaf: none._
