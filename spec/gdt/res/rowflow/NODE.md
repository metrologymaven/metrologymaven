---
id: GDT.RES.ROWFLOW
title: DMT row proposal, review and sign-off workflow
level: 3
parent: GDT.RES
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT.RES-005]
covers: []
depends_on: [GDT.RES.SCHEMA, CORE.JRN]
cli: [fid gdt propose-dmt-row, fid gdt review-dmt-row, fid gdt adopt-dmt-row]
mcp: [gdt_propose_dmt_row]
journal: [gdt.propose_dmt_row@1, gdt.review_dmt_row@1, gdt.adopt_dmt_row@1]
updated: 2026-11-16
---

# GDT.RES.ROWFLOW: DMT row proposal, review and sign-off workflow

## Purpose

Implements the D-009 process as commands and states: anyone (including an agent, on an agent branch) may propose a row change; a human author adopts it; two distinct credentialed DMT reviewers sign off; only then can a row ship in a released table. Interpretation disputes become an Interpretation Decision Record (IDR) attached to the row. This node owns the workflow mechanics, not who is credentialed (owned by `ECO`) nor the signature ceremony (owned by `TRUST`).

## Scope

**In:**
- `gdt.propose_dmt_row@1` (W1, agent branch allowed, provenance-tagged)
- `gdt.review_dmt_row@1` (W2, human reviewer only; records approve / request-changes)
- `gdt.adopt_dmt_row@1` (W3, human only: promotes a signed-off row into a released table)
- Review state machine `draft → under_review → signed_off` with the two-distinct-reviewer rule
- IDR records linked to rows
- Reviewer sign-off check used by `GDT.RES.LINT`

**Out** (and which node owns it instead):
- Credentialing and the DMT-reviewer role (owned by `ECO.COMM`)
- E-signatures (owned by `TRUST.ESIG`)
- Row content (owned by `GDT.RES.ROWASME`, `GDT.RES.ROWISO`)

## Requirements

- **REQ-GDT.RES.ROWFLOW-001** When an agent proposes a row change, the system shall store it on the agent branch with agent provenance and review state `draft`.
- **REQ-GDT.RES.ROWFLOW-002** A row shall reach `signed_off` only after approvals from two distinct human principals holding the DMT-reviewer role, neither of whom is the row's author.
- **REQ-GDT.RES.ROWFLOW-003** If any content field of a `signed_off` row changes, then its review state shall reset to `draft` and its reviewer list shall be cleared.
- **REQ-GDT.RES.ROWFLOW-004** Only a human principal shall run `gdt.adopt_dmt_row@1`, and only for rows in state `signed_off`.
- **REQ-GDT.RES.ROWFLOW-005** Every review and adoption shall be journaled with principal, role and timestamp, and the reviewer sign-off shall be what `GDT.RES.LINT` checks for REQ-GDT.RES-005.

## Interfaces

- **Offers:** the three commands above; `ReviewState` transitions; IDR type.
- **Consumes:** DMT schema; journal and branches (CORE.JRN); e-signature (TRUST.ESIG); reviewer role registry (ECO.COMM).

## Decomposition hints

STATE (state machine and invariants, a likely leaf), CMDS (three commands), IDR (record type and linking).

## Open questions

- Whether adoption into a released table is W3 (controlled record) or W2 depends on whether the DMT is a controlled record under the governance profiles (`TRUST.PROFILE`); W3 is assumed here.

## Children

_None yet: status `open`._
