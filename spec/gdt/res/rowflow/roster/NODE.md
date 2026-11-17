---
id: GDT.RES.ROWFLOW.ROSTER
title: DMT reviewer roster file and role lookup
level: 4
parent: GDT.RES.ROWFLOW
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWFLOW-002]
covers: []
depends_on: [CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWFLOW.ROSTER: DMT reviewer roster file and role lookup

## Purpose

D-009 requires two credentialed DMT reviewers per row, and D-016 makes the project buy a licence per paid author while community reviewers attest to their own copies. Someone must record who holds the reviewer role. This leaf defines the roster file (`dmt-reviewers/v1`), kept in the DMT data repository and changed only by pull request under the repository's review rules, plus a pure lookup the state machine uses. It records roles and their basis, never credentials documents or personal data beyond a display name.

## Scope

**In:** file format `dmt-reviewers/v1` (reviewer ID, display name, bound principal identities, dialects, basis `paid_author | partner | community`, valid-from and optional valid-until dates); loader and validator; `has_role(principal, dialect, at) -> Option<ReviewerId>`; roster digest recorded with each review.
**Out:** who may be added and how (governance, `ECO.COMM`); principal authentication (owned by `CORE.BUS`); review rules (owned by `GDT.RES.ROWFLOW.STATE`).

## Requirements

- **REQ-GDT.RES.ROWFLOW.ROSTER-001** The loader shall accept only `dmt-reviewers/v1` files with known fields, and fail with `roster_invalid{field}` otherwise.
- **REQ-GDT.RES.ROWFLOW.ROSTER-002** If two entries share a reviewer ID, or one principal identity is bound to two reviewer IDs, then loading shall fail with `roster_duplicate{id}`.
- **REQ-GDT.RES.ROWFLOW.ROSTER-003** `has_role(principal, dialect, at)` shall return the reviewer ID exactly when an entry binds that principal, lists that dialect, and `valid_from ≤ at` and (`valid_until` absent or `at < valid_until`).
- **REQ-GDT.RES.ROWFLOW.ROSTER-004** The roster digest shall be the SHA-256 of its RFC 8785 canonical JSON, and every review record shall carry the digest of the roster it was checked against.
- **REQ-GDT.RES.ROWFLOW.ROSTER-005** Free-text fields (display name) shall be at most 80 characters, and the format shall have no field for credential documents, contact details or licence keys.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/rowflow/roster_ok.json` (three reviewers; one ASME-only, one ISO-only, one expired on 2026-12-01), when `has_role` is queried for each at 2026-11-20 and 2026-12-02, then the results match the fixture's truth table.
- **AC-2** (error paths) Given `roster_dup_id.json` and `roster_dup_principal.json`, then `roster_duplicate`; given an unknown field `email`, then `roster_invalid{email}`.
- **AC-3** Given the same roster with entries in a different file order, then the digest is unchanged.

## Design

- **Registry command(s):** none; the roster is repository data read by the review command. Reading it is R0.
- **Data structures:** `Roster { version, entries: BTreeMap<ReviewerId, Entry> }`; `Entry { display_name, principals: Vec<PrincipalIdentity>, dialects: Vec<Dialect>, basis, valid_from: Date, valid_until: Option<Date> }`.
- **Algorithm:** serde with `deny_unknown_fields`; canonical JSON digest; index principals in a `BTreeMap`.
- **Errors:** `roster_invalid{field}`, `roster_duplicate{id}`, `field_too_long`.
- **Determinism:** D0.
- **Performance budget:** 1 000 entries load < 10 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::roster`).

## Test fixtures

Synthetic rosters with invented names (CC0, in-repo).

## Build prompt

> In crate `fid-gdt`, add module `dmt_flow::roster`. Define the `dmt-reviewers/v1` format (reviewer ID, display name ≤ 80 chars, bound principal identities, dialects, basis `paid_author|partner|community`, `valid_from`, optional `valid_until`; no other fields) with serde `deny_unknown_fields`; loader errors `roster_invalid{field}`, `roster_duplicate{id}` (duplicate IDs or one principal bound twice), `field_too_long`; `has_role(principal, dialect, at) -> Option<ReviewerId>` (valid_from ≤ at < valid_until); digest = SHA-256 of RFC 8785 canonical JSON, order-independent. Implement the `RosterView` trait used by `dmt_flow::state`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, no personal data beyond display name.

## Open questions

- Who approves roster changes (maintainers of GDT only, or a separate DMT committee) is governance for `ECO.COMM` and `GOVERNANCE.md`.

## Children

_Leaf: none._
