---
id: GDT.RES.ROWFLOW.STATE
title: Row revisions, review state machine and invariants
level: 4
parent: GDT.RES.ROWFLOW
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-gdt]
satisfies: [REQ-GDT.RES.ROWFLOW-002, REQ-GDT.RES.ROWFLOW-003, REQ-GDT.RES.ROWFLOW-005]
covers: []
depends_on: [GDT.RES.SCHEMA, CORE.JRN.CANON]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.RES.ROWFLOW.STATE: Row revisions, review state machine and invariants

## Purpose

The pure core of the DMT workflow: the types for a row revision, its review records and IDRs, and one transition function that every command calls. All D-009 and D-016 rules (human author, two distinct non-author reviewers with the role, edition-held attestation for every cited edition, reset on content change) are invariants of this function, so the commands cannot drift from them and the rules are testable without a journal.

## Scope

**In:** `RowRevision { key, revision, content_digest, author, author_kind, provenance, derived_from, state, reviews, idrs }`; `ReviewRecord { reviewer, decision, edition_attestations, idr, at }`; `Idr { id, question_code, positions, status }`; `transition(revision, event, roster_view) -> Result<RowRevision, FlowError>`; the mapping from a `signed_off` revision to the schema's `ReviewState::signed_off{reviewers}`.
**Out:** commands and journaling (owned by `GDT.RES.ROWFLOW.PROPOSE`, `GDT.RES.ROWFLOW.REVIEW`, `GDT.RES.ROWFLOW.ADOPT`); the roster (owned by `GDT.RES.ROWFLOW.ROSTER`); schema types (owned by `GDT.RES.SCHEMA`).

## Requirements

- **REQ-GDT.RES.ROWFLOW.STATE-001** A revision proposed by an agent principal shall have `author_kind: agent`, `provenance: agent`, state `draft`, and shall not accept any review event until a human principal re-proposes it as a new revision with `derived_from` set (human authorship).
- **REQ-GDT.RES.ROWFLOW.STATE-002** The first valid review event on a human-authored `draft` revision shall move it to `under_review`.
- **REQ-GDT.RES.ROWFLOW.STATE-003** A revision shall move to `signed_off` exactly when it holds `approve` reviews from two distinct reviewers, each a human principal holding the DMT-reviewer role for the row's dialect at the review time, neither equal to the author, with no later `request_changes`.
- **REQ-GDT.RES.ROWFLOW.STATE-004** A review shall be rejected with `attestation_missing{designator, edition}` unless it attests "edition held" for every (designator, edition) the row cites; the only exception shall be a row with status `unsupported` and an empty chain, which may carry attestation `not_required_placeholder`.
- **REQ-GDT.RES.ROWFLOW.STATE-005** A `request_changes` review shall move the revision to `draft` and clear its approvals; it may open an IDR whose position statements are each at most 280 characters.
- **REQ-GDT.RES.ROWFLOW.STATE-006** Any new revision of a key whose content digest differs from the previous revision shall start in `draft` with no reviews, so a content change to a `signed_off` row resets it (REQ-GDT.RES.ROWFLOW-003).
- **REQ-GDT.RES.ROWFLOW.STATE-007** The transition function shall reject a self-review (`reviewer_is_author`), a duplicate reviewer (`duplicate_reviewer`), a reviewer without the role (`reviewer_not_in_roster`) and a non-human reviewer (`reviewer_not_human`).
- **REQ-GDT.RES.ROWFLOW.STATE-008** A `signed_off` revision shall map to the schema's `ReviewState::signed_off{reviewers}` with both reviewer IDs sorted, which is the field `GDT.RES.LINT` rule L5 checks.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/rowflow/happy.json` (human author, reviewers r1 and r2 in the roster with attestations for every cited edition), when the events are applied, then the states are draft → under_review → signed_off and the mapped `ReviewState` lists [r1, r2].
- **AC-2** (error paths) Given fixtures `self_review.json`, `dup_reviewer.json`, `not_in_roster.json`, `agent_reviewer.json`, `no_attestation.json`, then the matching error codes are returned and the revision is unchanged.
- **AC-3** Given an agent-authored revision, when a review event is applied, then `agent_authored_revision`; after a human re-proposal with `derived_from`, the review is accepted.
- **AC-4** Given a signed-off revision and a new revision with a different content digest, then the new revision is `draft` with no reviews.
- **AC-5** Given an `unsupported` placeholder row with an empty chain, then attestation `not_required_placeholder` is accepted; given a `supported` row, the same attestation is rejected.
- **AC-6** Given an IDR position of 281 characters, then `field_too_long`.
- **AC-7** (determinism) Applying the same event list twice gives byte-identical canonical JSON.

## Design

- **Registry command(s):** none (pure library used by the three commands).
- **Data structures:** as in Scope; `Decision { Approve, RequestChanges }`; `Attestation { EditionHeld{designator, edition} | NotRequiredPlaceholder }`; `RosterView` (trait from ROSTER: `has_role(reviewer, dialect, at)`).
- **Algorithm:** a total match over (state, event); content digest = SHA-256 of the RFC 8785 canonical JSON of the row's content fields (schema digest rules).
- **Errors:** `agent_authored_revision`, `attestation_missing{designator, edition}`, `reviewer_is_author`, `duplicate_reviewer`, `reviewer_not_in_roster`, `reviewer_not_human`, `field_too_long`, `bad_transition{state, event}`.
- **Determinism:** D0.
- **Performance budget:** trivial per event.
- **Crate:** `fid-gdt` (module `dmt_flow::state`).

## Test fixtures

Synthetic rows and event lists (invented keys and clause numbers, no standards text, CC0, in-repo); a synthetic roster.

## Build prompt

> In crate `fid-gdt`, add module `dmt_flow::state`. Define `RowRevision`, `ReviewRecord`, `Attestation { EditionHeld{designator, edition}, NotRequiredPlaceholder }`, `Idr` (positions ≤ 280 chars each) and `transition(rev, event, roster: &dyn RosterView) -> Result<RowRevision, FlowError>` enforcing: agent-authored revisions accept no review until a human re-proposes with `derived_from`; first valid review moves draft → under_review; two `approve` reviews from distinct human reviewers holding the role for the row's dialect, neither the author, move to `signed_off`; `request_changes` returns to draft and clears approvals (optionally opening an IDR); every review must attest "edition held" for every cited (designator, edition) except `NotRequiredPlaceholder` on an `unsupported` row with an empty chain; new content digest → new draft revision with no reviews. Map `signed_off` to the schema's `ReviewState::signed_off{reviewers}` with sorted IDs. Errors as listed in the acceptance criteria. Tests per the acceptance criteria with synthetic fixtures only. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), no std `HashMap`/`HashSet`, never include standards text in code, comments or fixtures.

## Open questions

- Whether a reviewer from the same organisation as the author may count as one of the two reviewers is a governance choice for `ECO.COMM`; this leaf does not check affiliation.

## Children

_Leaf: none._
