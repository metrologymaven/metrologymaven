---
id: OUTR.BENCH.CASE
title: User case studies
level: 3
parent: OUTR.BENCH
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BENCH-007, REQ-OUTR.BENCH-008]
covers: []
depends_on: [TRUST.VERIFY, OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BENCH.CASE: User case studies

## Purpose

Case studies written with organisations that use the platform, published only with their written consent, with every number either verifiable from a result package or clearly marked as the organisation's own statement. It is separate because it involves third parties, consent records and confidential data.

## Scope

**In:**
- Consent process and consent record storage
- Case study template with a claims table
- Verification of quantitative claims with `fid-verify` result packages
- Withdrawal process

**Out** (and which node owns it instead):
- The verifier (owned by `TRUST.VERIFY`)
- Paid partnerships or sales references (business documents outside this tree)

## Requirements

- **REQ-OUTR.BENCH.CASE-001** A case study shall be published only when a written consent from the named organisation, covering the exact text to be published, is on file.
- **REQ-OUTR.BENCH.CASE-002** Each quantitative claim in a case study shall either link a result package that `fid-verify` verifies or be labelled as the organisation's own statement.
- **REQ-OUTR.BENCH.CASE-003** If the organisation withdraws consent, then the case study shall be removed from the website within 14 days [unverified target].
- **REQ-OUTR.BENCH.CASE-004** Every case study shall pass the OUTR.BRAND claim lint with zero errors.
- **REQ-OUTR.BENCH.CASE-005** Consent records shall be stored outside the public repository, accessible to maintainers only.

## Interfaces

- **Offers:** case study pages (hosted by OUTR.SITE).
- **Consumes:** TRUST.VERIFY; OUTR.BRAND.LINT.

## Decomposition hints

CONSENT (consent and withdrawal), TMPL (template and claims table), VERIFY (claim verification).

## Open questions

- Phase P1b: a case study needs a real user of a released app.

## Children

_None yet: status `open`._
