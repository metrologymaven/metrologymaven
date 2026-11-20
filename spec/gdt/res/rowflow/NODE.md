---
id: GDT.RES.ROWFLOW
title: DMT row proposal, review and sign-off workflow
level: 3
parent: GDT.RES
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-005, REQ-GDT.RES-010]
covers: []
depends_on: [GDT.RES.SCHEMA, GDT.RES.LINT, CORE.JRN, CORE.BUS]
cli: [fid gdt propose-dmt-row, fid gdt review-dmt-row, fid gdt adopt-dmt-row]
mcp: [gdt_propose_dmt_row, gdt_review_dmt_row, gdt_adopt_dmt_row]
journal: [gdt.propose_dmt_row@1, gdt.review_dmt_row@1, gdt.adopt_dmt_row@1]
updated: 2026-11-19
---

# GDT.RES.ROWFLOW: DMT row proposal, review and sign-off workflow

## Purpose

Implements the D-009 process as commands and states: anyone (including an agent, on an agent branch) may propose a row change; a human author adopts it; two distinct credentialed DMT reviewers sign off; only then can a row ship in a released table. Interpretation disputes become an Interpretation Decision Record (IDR) attached to the row. This node owns the workflow mechanics, not who is credentialed (owned by `ECO`) nor the signature ceremony (owned by `TRUST`).

## Scope

**In:**
- `gdt.propose_dmt_row@1` (W1, agent branch allowed, provenance-tagged)
- `gdt.review_dmt_row@1` (W2, human only: records approve / request-changes)
- `gdt.adopt_dmt_row@1` (W3, human only: promotes a signed-off row into a released table)
- Review state machine `draft → under_review → signed_off` with the two-distinct-reviewer rule
- IDR records linked to rows
- Reviewer sign-off check used by `GDT.RES.LINT`
- The "edition held" attestation on every review record (D-016)
- The DMT reviewer roster file format and lookup (no other node owns it yet)

**Out** (and which node owns it instead):
- Credentialing policy and the community DMT-reviewer role as a governance process (owned by `ECO.COMM`; see Open questions)
- E-signatures over a released table package (owned by `TRUST.ESIG`, which reads the released table as an artifact)
- Row content (owned by `GDT.RES.ROWASME`, `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.RES.ROWFLOW-001** When an agent proposes a row change, the system shall store it on the agent branch with agent provenance and review state `draft`.
- **REQ-GDT.RES.ROWFLOW-002** A row shall reach `signed_off` only after approvals from two distinct human principals holding the DMT-reviewer role, neither of whom is the row's author.
- **REQ-GDT.RES.ROWFLOW-003** If any content field of a `signed_off` row changes, then its review state shall reset to `draft` and its reviewer list shall be cleared.
- **REQ-GDT.RES.ROWFLOW-004** Only a human principal shall run `gdt.adopt_dmt_row@1`, and only for rows in state `signed_off`.
- **REQ-GDT.RES.ROWFLOW-005** Every review and adoption shall be journaled with principal, role and timestamp, and the reviewer sign-off shall be what `GDT.RES.LINT` checks for REQ-GDT.RES-005.
- **REQ-GDT.RES.ROWFLOW-006** Every review shall name the content digest the reviewer read, and if it differs from the revision's content digest, then the review shall fail with `stale_review` and nothing shall be journaled, so a sign-off is bound to the exact content it covers (D-045).

## Interfaces

- **Offers:** the three commands above; `RowRevision`, `ReviewRecord` (with edition-held attestation) and `Idr` types; the reviewer roster format `dmt-reviewers/v1`; the released `DmtTable` file with its digest, which `TRUST` may sign and `GDT.RES.ROWASME.GATE` checks.
- **Consumes:** DMT schema (GDT.RES.SCHEMA); release-mode lint (GDT.RES.LINT); journal and branches (CORE.JRN); principals and `human_only` enforcement (CORE.BUS).

## Decomposition hints

STATE (state machine and invariants, a likely leaf), CMDS (three commands), IDR (record type and linking).

## Open questions

- **D-045 (accepted 2026-11-19):** REQ-GDT.RES.ROWFLOW-006 binds each review to the content digest the reviewer read (`stale_review`). The public reviewer role, its qualification criteria and conflict-of-interest rules (including the affiliation question on `GDT.RES.ROWFLOW.STATE`) are now owned by `OUTR.GDTC.REVIEW`, answering the cross-domain question below.
- Adoption into a released table is W3 (controlled record) under the assumption that a released DMT table is a controlled record; D-031 makes every `*.adopt_*` command `human_only`, so the class only matters for W2 versus W3 audit treatment. If `TRUST.PROFILE` later says the DMT is not controlled, `gdt.adopt_dmt_row@1` becomes W2 and nothing else changes.
- The earlier dependency on `TRUST.ESIG` is removed: GDT is below TRUST in the L2 order (D-034), so GDT may not depend on it. Review approvals are journaled human-only commands; a signed release of the table is TRUST's job, reading the released table file as an artifact.
- Cross-domain: no `ECO.COMM` node owns the DMT reviewer credentialing process (D-009's community reviewer role and partner institutions). `GDT.RES.ROWFLOW.ROSTER` defines the roster file and lookup so the workflow can run; `ECO.COMM` should own the policy for adding and removing reviewers when it is expanded.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.RES.ROWFLOW.STATE | Row revisions, review state machine and invariants (leaf) | 002 003 005 | none |
| GDT.RES.ROWFLOW.ROSTER | DMT reviewer roster file and role lookup (leaf) | 002 | none |
| GDT.RES.ROWFLOW.PROPOSE | Propose a DMT row revision (leaf) | 001 003 | none |
| GDT.RES.ROWFLOW.REVIEW | Review a DMT row with edition-held attestation (leaf) | 002 005 006 | none |
| GDT.RES.ROWFLOW.ADOPT | Adopt signed-off rows into a released table (leaf) | 004 005 | none |
