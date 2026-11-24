---
id: TRUST.PROFILE.MAP
title: Regulated profile control mapping document
level: 3
parent: TRUST.PROFILE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE-007]
covers: []
depends_on: [TRUST.PROFILE.DEFS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [docs/trust, ci/trust]
built_pr: 480
updated: 2026-11-22
---

# TRUST.PROFILE.MAP: Regulated profile control mapping document

## Purpose

A published, project-written document that maps each control the Regulated profile provides (signature manifestation, meaning, linkage, sequence, audit trail, release gating, hardware keys) to the regulation designator and section number it relates to, for example 21 CFR Part 11 and EU GMP Annex 11 [unverified section numbers]. It cites by designator and section only, never quotes, and states plainly that using the profile does not by itself make a system compliant. A machine-readable table drives a CI check that every mapped control names a real spec requirement.

## Scope

**In:**
- `docs/trust/regulated-mapping.md` (CC-BY-4.0, original text)
- `docs/trust/regulated-mapping.csv`: control ID, short description, spec requirement IDs, regulation designator, section number, review status
- Mandatory disclaimer paragraph
- CI check that every listed requirement ID exists in the spec tree and every row has a review status

**Out:**
- Implementing the controls (`TRUST.ESIG`, `TRUST.SIGN`, `TRUST.PROFILE.GATE`, `CORE.JRN`)
- Validation services or compliance claims (out of scope for the open core)

## Requirements

- **REQ-TRUST.PROFILE.MAP-001** Each mapping row shall cite a regulation by designator and section number only, with a description written by the project in its own words.
- **REQ-TRUST.PROFILE.MAP-002** The document shall contain a disclaimer stating that the profile supports but does not by itself establish compliance, and that validation remains the user's responsibility.
- **REQ-TRUST.PROFILE.MAP-003** Every row shall reference at least one existing spec requirement ID, and CI shall fail when a referenced ID does not exist.
- **REQ-TRUST.PROFILE.MAP-004** Every row shall carry a review status of `draft` or `reviewed`, and a row may be `reviewed` only with the name of a reviewer with relevant regulatory experience recorded in the PR.

## Acceptance criteria

- **AC-1** Given `docs/trust/regulated-mapping.csv`, when `ci/trust/check-mapping` runs, then every requirement ID resolves in the spec tree and every row has a review status.
- **AC-2** (error path) Given a row referencing `REQ-TRUST.ESIG-099`, then the check fails naming the row.
- **AC-3** Given the rendered document, then it contains the disclaimer paragraph and no quoted regulatory text longer than a section title (manual review checklist item recorded in the PR).

## Design

- **Registry command(s):** none.
- **Data structures:** CSV columns `control_id, description, requirement_ids, designator, section, review_status`.
- **Algorithm:** CI script parses the CSV and greps requirement IDs from `spec/**/NODE.md` (or the generated index).
- **Errors:** CI failure naming the row.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** none (documentation + CI script under `ci/trust/`; owned with `fid-profile`).

## Test fixtures

- The CSV itself; a negative test CSV under `ci/trust/negative/`.

## Build prompt

> Write `docs/trust/regulated-mapping.md` and `docs/trust/regulated-mapping.csv` for the Regulated governance profile. For each control the profile provides (signature manifestation, meaning, signature-to-record linkage, ordered multi-signer sequence, audit trail via the hash-chained journal, release gating, hardware-backed keys, user verification), give the project's own one-line description, the spec requirement IDs that implement it, and the regulation designator and section number (21 CFR Part 11, EU GMP Annex 11) it relates to. Cite designators and section numbers only; never quote regulatory text. Mark every row `draft` until a reviewer with regulatory experience signs off in a PR. Include a disclaimer that the profile does not by itself make a system compliant. Add `ci/trust/check-mapping` that fails when a requirement ID does not exist or a row lacks a review status, with a negative test CSV. Tests: the three acceptance criteria. Constraints: AGENTS.md rule on standards text; CC-BY-4.0 for the doc.

## Open questions

- Section numbers for Part 11 and Annex 11 are not verified here [unverified]; the first reviewer confirms them.

## Children

_Leaf: none._
