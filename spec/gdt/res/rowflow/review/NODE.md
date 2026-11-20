---
id: GDT.RES.ROWFLOW.REVIEW
title: Review a DMT row with edition-held attestation
level: 4
parent: GDT.RES.ROWFLOW
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.RES.ROWFLOW-002, REQ-GDT.RES.ROWFLOW-005, REQ-GDT.RES.ROWFLOW-006]
covers: []
depends_on: [GDT.RES.ROWFLOW.STATE, GDT.RES.ROWFLOW.ROSTER, CORE.BUS]
side_effect: W2
determinism: D0
cli: [fid gdt review-dmt-row]
mcp: [gdt_review_dmt_row]
journal: [gdt.review_dmt_row@1]
updated: 2026-11-19
---

# GDT.RES.ROWFLOW.REVIEW: Review a DMT row with edition-held attestation

## Purpose

The human act of reviewing a DMT row: approve or request changes, attest that the reviewer holds a lawful copy of every standard edition the row cites (D-016), and optionally open an Interpretation Decision Record when the reviewer disputes the reading. The command is `human_only` (D-031): agents and scripts are refused whatever channel they use, and MCP projects it as a request a human must act on. Two approvals from distinct roster reviewers sign the row off (D-009).

## Scope

**In:** `gdt.review_dmt_row@1` (W2, `human_only: true`); params: revision ID, reviewed content digest, decision, attestations, optional IDR; roster lookup with the roster digest recorded; transition through STATE; journal event with principal, reviewer ID, role, decision, attestations and timestamp.
**Out:** the rules themselves (owned by `GDT.RES.ROWFLOW.STATE`); the roster (owned by `GDT.RES.ROWFLOW.ROSTER`); signatures (owned by `TRUST`).

## Requirements

- **REQ-GDT.RES.ROWFLOW.REVIEW-001** The command shall be registered with `human_only: true`, so the bus refuses agent and script principals and MCP projects it as request-only.
- **REQ-GDT.RES.ROWFLOW.REVIEW-002** The command shall resolve the caller's principal to a reviewer ID through the roster for the row's dialect, and fail with `reviewer_not_in_roster` if none matches.
- **REQ-GDT.RES.ROWFLOW.REVIEW-003** The command shall require an attestation for every cited (designator, edition) as STATE defines, and fail with `attestation_missing` otherwise, journaling nothing.
- **REQ-GDT.RES.ROWFLOW.REVIEW-004** A committed review shall journal one `gdt.review_dmt_row@1` event carrying principal, reviewer ID, role `dmt_reviewer`, decision, attestations, roster digest, revision content digest, optional IDR, and UTC timestamp.
- **REQ-GDT.RES.ROWFLOW.REVIEW-005** When the review is the second valid approval, the result shall report the new state `signed_off` and both reviewer IDs.
- **REQ-GDT.RES.ROWFLOW.REVIEW-006** The command shall take the content digest the reviewer read, and if it differs from the revision's content digest, then it shall fail with `stale_review`, review nothing and journal nothing (D-045: a sign-off is bound to the exact content it covers, as in AUTO.REC.ADOPT).
- **REQ-GDT.RES.ROWFLOW.REVIEW-007** If any text the review carries (IDR positions, notes) contains a quoted span (text in quotation marks or a block quote) of any length in the same field as a standards designator, then the command shall fail with `standards_quote` naming the field and journal nothing, so no verbatim standards text enters the journal (D-045, CONSTITUTION §5).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/rowflow/review_flow.json` (human-authored draft, roster with r1 and r2), when r1 and then r2 run `fid gdt review-dmt-row --revision <id> --reviewed-digest <d> --approve --attest "<designator>:<edition>" --json` in human-upgraded sessions, then the second result reports `signed_off` with [r1, r2] and two journal events exist.
- **AC-2** (error path) Given an MCP agent channel, then the call becomes a pending human request and nothing is reviewed; given a plain `script` CLI session, then the bus refuses it.
- **AC-3** (error path) Given a reviewer missing from the roster, then `reviewer_not_in_roster` and nothing journaled; given a missing attestation, then `attestation_missing`.
- **AC-4** Given `--request-changes --idr <file>`, then the revision returns to `draft`, an IDR is attached, and the event carries it.
- **AC-5** (replay) Exact replay of AC-1 gives `IDENTICAL`.
- **AC-6** (error path, D-045) Given the AC-1 fixture and `--reviewed-digest` set to the digest of a different revision of the same key, when r1 runs the review, then it fails with `stale_review`, the revision state is unchanged and no event is journaled.
- **AC-7** (error path, D-045) Given `--request-changes --idr <file>` whose position reads `as "lorem ipsum" in ISO 99999` (an invented designator), then `standards_quote`, the revision is unchanged and nothing is journaled; the same position without quotation marks is accepted.

## Design

- **Registry command(s):** `gdt.review_dmt_row@1`, W2, `human_only: true` (MCP request-only). Params: `revision: RevisionId`, `reviewed_digest: Sha256` (the content digest the reviewer read), `decision: approve | request_changes`, `attestations: [Attestation]`, `idr: Option<IdrDraft>`. Result: `{ revision, state, reviewers }`. `--json`.
- **Data structures:** from STATE and ROSTER.
- **Algorithm:** load revision and the pinned roster → `has_role` → `state::transition(Review{...})` → commit.
- **Errors:** `standards_quote{field}`, `stale_review`, `reviewer_not_in_roster`, `attestation_missing`, `reviewer_is_author`, `duplicate_reviewer`, `agent_authored_revision`, `field_too_long`, `unknown_revision`.
- **Determinism:** D0.
- **Performance budget:** < 20 ms [unverified].
- **Crate:** `fid-gdt` (module `dmt_flow::cmd`).

## Test fixtures

Synthetic rows and roster (CC0, in-repo); attestations naming invented editions only.

## Build prompt

> In crate `fid-gdt`, module `dmt_flow::cmd`, implement registry command `gdt.review_dmt_row@1` (W2, `human_only: true`, so the bus refuses agent and script principals and MCP projects it request-only; CLI `fid gdt review-dmt-row`; MCP `gdt_review_dmt_row`; one journal event; `--json`). Params: revision, `reviewed_digest` (fail with `stale_review`, journaling nothing, when it is not the revision's content digest), decision (`approve` | `request_changes`), attestations (one "edition held" per cited designator and edition, or `not_required_placeholder` for unsupported placeholder rows), optional IDR. Resolve the principal to a reviewer through `dmt_flow::roster` for the row's dialect; apply `dmt_flow::state::transition`; journal principal, reviewer ID, role `dmt_reviewer`, decision, attestations, roster digest, content digest, IDR, UTC time. Report `signed_off` and both reviewers on the second approval. Refuse any quoted span (quotation marks or block quote, any length) in a review text field that also contains a standards designator, with `standards_quote{field}`, journaling nothing. Tests per the acceptance criteria (two-reviewer happy path, stale reviewed digest, quoted standards text, agent and script refusal, roster and attestation errors, request-changes with IDR, exact replay). Constraints: CONSTITUTION §3 (human-only commands, principals per D-012), §8 (no `unwrap`/`expect`/`panic`/`unsafe`), never store standards text.

## Open questions

- An attestation is a statement by the reviewer, not proof of a licence; D-016 accepted that model. Whether the Regulated profile wants the attestation also covered by a signature is for `TRUST.PROFILE`.

## Children

_Leaf: none._
