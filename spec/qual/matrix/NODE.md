---
id: QUAL.MATRIX
title: Compliance matrix and regulated-claim wording
level: 2
parent: QUAL
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-002, REQ-QUAL-007]
covers: []
depends_on: [QUAL.EVID, QUAL.VAP, TRUST.PROFILE.MAP, ECO.RULES.VOCAB, OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- **Consumes:** evidence-bundle member names and digest (`QUAL.EVID`); VAP checklist row IDs (`QUAL.VAP`); `TRUST.PROFILE.MAP` CSV; rule files of `ECO.RULES.VOCAB` and `OUTR.BRAND.LINT`.

## Decomposition hints

DATA (matrix schema and rows per framework), CHECK (reference and import check), PAGE (rendering), TERMS (regulated-term list and consistency test).

## Open questions

- The two lints currently list different word forms (the outreach lint includes "traceable"; the product lint does not). REQ-007 will fail until `ECO.RULES.VOCAB` adds it; cross-domain.
- Which frameworks are in the P1a matrix is a maintainer choice; the concept's list is broad and clause IDs need expert review [unverified clause IDs].
- Frameworks that have been revised (for example AS9100 to IA9100) need an edition policy for rows.

## Children

_None yet: status `open`._
