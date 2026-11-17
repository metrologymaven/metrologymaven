---
id: QUAL.MATRIX
title: Compliance matrix and regulated-claim wording
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-002, REQ-QUAL-007]
covers: []
depends_on: []
uses_artifacts: [QUAL.EVID.LAYOUT, QUAL.EVID.ASSEMBLE, TRUST.PROFILE.MAP, ECO.RULES.VOCAB, OUTR.BRAND.LINT, ECO.CI.WF.TESTRPT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.MATRIX: Compliance matrix and regulated-claim wording

## Purpose

Publishes, per release, the compliance matrix that maps customer frameworks (AS9100, IATF 16949, ISO 13485 and FDA QMSR, ISO/IEC 17025, 21 CFR Part 11 and others) clause by clause to the platform controls, the tests that check them and the evidence that shows the result, by clause ID only. It also owns the project's regulated-claim wording policy: the list of terms that may never appear unqualified, and the check that the product-string lint and the outreach lint both enforce that list. The two belong together because both decide what the project may say about compliance.

## Scope

**In:**
- Matrix data file: framework designator and edition, clause ID, platform control, spec requirement IDs, test or check ID, evidence member, review status
- Rendered matrix page per release, citing the evidence-bundle digest
- CI check that every cited requirement ID, test ID and evidence member exists
- The customer-responsibility statement
- The regulated-term list (for example certified, compliant, conformant, validated, traceable and their inflections) with the qualifier each needs
- A consistency test that the rule files of `ECO.RULES.VOCAB` and `OUTR.BRAND.LINT` both contain every term in that list
- Running both lints over QUAL's own published artefacts (VAP, matrix, security and CRA documents)

**Out** (and which node owns it instead):
- The Part 11 and Annex 11 rows for the Regulated profile (owned by `TRUST.PROFILE.MAP`; this matrix imports them)
- The lint engines and their rule-file formats (owned by `ECO.RULES.VOCAB` for product strings and `OUTR.BRAND.LINT` for outreach)
- Evidence content (owned by `QUAL.EVID`) and checklists in the VAP (owned by `QUAL.VAP`)
- Any compliance determination for a customer (never made)

## Requirements

- **REQ-QUAL.MATRIX-001** Each release shall publish a compliance matrix in which every row names a framework designator with edition, a clause ID, a platform control, at least one spec requirement ID and at least one evidence member.
- **REQ-QUAL.MATRIX-002** The matrix shall contain no text quoted from any standard or regulation, and each row description shall be written by the project.
- **REQ-QUAL.MATRIX-003** If a row cites a requirement ID, test ID or evidence member that does not exist in the spec tree or the release's evidence bundle, then the matrix check shall fail and name the row.
- **REQ-QUAL.MATRIX-004** The matrix shall state, on its first page and in its data file header, that it describes evidence the project supplies and that compliance decisions belong to the customer.
- **REQ-QUAL.MATRIX-005** Each row shall carry a review status of `draft` or `reviewed`, and a row may be `reviewed` only when a named reviewer with relevant regulatory experience is recorded in the PR.
- **REQ-QUAL.MATRIX-006** The matrix shall import the rows of `TRUST.PROFILE.MAP` by control ID rather than restating them, and the check shall fail if an imported row is missing.
- **REQ-QUAL.MATRIX-007** The project shall keep one versioned regulated-term list, and a CI test shall fail if any listed term is absent from the rule file of `ECO.RULES.VOCAB` or of `OUTR.BRAND.LINT`.
- **REQ-QUAL.MATRIX-008** Every QUAL-published document shall pass the outreach claim lint with zero errors before release.

## Interfaces

- **Offers:** matrix data file and rendered page per release; the regulated-term list; the matrix and term-consistency checks (run by `ECO.CI`).
- **Consumes (all as artifacts):** evidence-statement schema and per-release statement (`QUAL.EVID.LAYOUT`, `QUAL.EVID.ASSEMBLE`); CI test report (`ECO.CI.WF.TESTRPT`); the VAP checklists link to matrix rows, not the reverse, so no VAP dependency remains; `TRUST.PROFILE.MAP` CSV; rule files of `ECO.RULES.VOCAB` and `OUTR.BRAND.LINT`.

## Decomposition hints

DATA (matrix schema and rows per framework), CHECK (reference and import check), PAGE (rendering), TERMS (regulated-term list and consistency test).

## Split rationale

The split separates the matrix data and its local rules (DATA) from checks that need other nodes' artifacts (CHECK), from the per-release publication (PAGE), and from the regulated-term list that governs wording across the project (TERMS). TERMS sits here because, like the matrix, it decides what the project may say about compliance.

Alternatives considered:
- **DATA and CHECK in one leaf.** Rejected: the reference check reads four external artifacts and runs at release time, while the schema check runs on every PR to the matrix.
- **TERMS as its own QUAL subsystem.** Rejected at level 2 (see `QUAL` split rationale); as a leaf it is size S.

## Open questions

- The two lints listed different word forms ("traceable"); `ECO.RULES.VOCAB` now says its product rule set includes it. `QUAL.MATRIX.TERMS` will confirm.
- Which frameworks are in the P1a matrix is a maintainer choice; clause IDs need expert review [unverified clause IDs].
- Frameworks that have been revised (for example AS9100 to IA9100) follow the one-edition-per-row policy in `QUAL.MATRIX.DATA`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.MATRIX.DATA | Compliance matrix schema, rows and review rules | 001 002 004 005 | none |
| QUAL.MATRIX.CHECK | Matrix reference and profile-map import check | 003 006 | none |
| QUAL.MATRIX.PAGE | Per-release matrix rendering | 001 004 | none |
| QUAL.MATRIX.TERMS | Regulated-term list, cross-lint consistency and QUAL document lint | 007 008 | none |
