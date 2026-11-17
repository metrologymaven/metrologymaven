---
id: KRN.CONS.ELEM.SET
title: Constructed sets
level: 4
parent: KRN.CONS.ELEM
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-KRN.CONS.ELEM-003, REQ-KRN.CONS.ELEM-005]
covers: [PCD.CON.03]
depends_on: [KRN.CONS.DECL, KRN.MAN.CARD]
side_effect: W2
determinism: D0
cli: [fid construct make-set]
mcp: [construct_make_set]
journal: [construct.make_set@1]
updated: 2026-11-17
---

# KRN.CONS.ELEM.SET: Constructed sets

## Purpose

An ordered, typed group of elements that acts as one input to other constructions (a hole pattern used for a bolt-circle fit, a set of planes used for a datum). The set is itself a construction whose declared inputs are its members in order, so changing membership or order changes its dependency declaration and recomputes everything built on it.

## Scope

**In:** `construct.make_set.ordered@1.0.0`; member-kind policy (`homogeneous` with a required kind, or `mixed`); duplicate and size checks; the set element and its canonical encoding; the command `construct.make_set@1`; a spec card.
**Out:** using sets as inputs (`KRN.CONS.ELEM.FITC` and other constructions); selection of members by query or lasso (`GEO.SEL`); graph recompute (`CORE.DEP`).

## Requirements

- **REQ-KRN.CONS.ELEM.SET-001** The module shall build a set whose members are the given element IDs in the given order, and whose dependency declaration lists them in that order.
- **REQ-KRN.CONS.ELEM.SET-002** Changing the order or membership of a set shall change its dependency declaration bytes and its output digest.
- **REQ-KRN.CONS.ELEM.SET-003** If a member appears twice, the set is empty or has more than 65 535 members, or a member's kind violates the `homogeneous` policy, then the build shall return `DuplicateMember{index}`, `InputCount` or `WrongInputKind{index}`.
- **REQ-KRN.CONS.ELEM.SET-004** When the command is committed, the system shall journal one `construct.make_set@1` event with algorithm ID, input element IDs, parameter digest and output digest.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cons/set-v1` (12 circle element refs), when `fid construct make-set --kind circle --json` runs, then the set lists the 12 members in order and its declaration encodes them in order.
- **AC-2** Given the same members with two swapped, then the declaration bytes and output digest differ from AC-1.
- **AC-3** (error path) Given a duplicate member or a plane in a `homogeneous: circle` set, then the build returns `DuplicateMember{index}` or `WrongInputKind{index}` and journals nothing.
- **AC-4** Given a committed AC-1 run, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `construct.make_set@1`, W2 (dry-run R1). Params: ordered member handles, `policy: homogeneous(kind) | mixed`. Result: set element `{ members: Vec<ElementRef>, policy }`.
- **Algorithm:** duplicate check by sorting a copy of the IDs (no hash sets); kind check per member.
- **Errors:** `DuplicateMember`, `InputCount`, `WrongInputKind`.
- **Determinism:** D0.
- **Performance budget:** 65 535 members in < 5 ms [unverified until measured].
- **Crate:** `fid-construct` (module `elem_set`).

## Test fixtures

`fixtures/cons/set-v1`: synthetic element refs, CC-BY-4.0.

## Build prompt

> In crate `fid-construct` (pure Rust, no I/O, no `unsafe`; depends on `fid-num` and `fid-fit` only), add module `elem_set` implementing the `Construction` trait for `construct.make_set.ordered@1.0.0`: members are element refs in caller order, declared in that order; policy `homogeneous(kind)` or `mixed`; reject duplicates (sort a copy, no HashSet), empty sets and sets over 65 535 members, and wrong kinds. Add the registry data row `construct.make_set@1` (W2, dry-run R1, D0; CLI `fid construct make-set`; MCP `construct_make_set`; one journal event with algorithm ID, input element IDs, parameter digest, output digest) and a spec card. Tests: `fixtures/cons/set-v1` order preservation, swapped-order digest change, duplicate and wrong-kind errors, exact replay IDENTICAL. Deny `unwrap`/`expect`/`panic`; no HashMap/HashSet. Acceptance: AC-1..AC-4 of spec node KRN.CONS.ELEM.SET.

## Open questions

- `KRN.CONS.DECL`'s `ConstructedElement` has Feature and Value variants only; a set needs a `Set` variant (or sets are declarations without geometry). Raised for the DECL owner.

## Children

_Leaf: none._
