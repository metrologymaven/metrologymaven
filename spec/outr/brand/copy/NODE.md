---
id: OUTR.BRAND.COPY
title: Copied-text detection rule
level: 3
parent: OUTR.BRAND
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND-005]
covers: []
depends_on: [OUTR.BRAND.LINT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.COPY: Copied-text detection rule

## Purpose

A claim-lint rule that flags passages in outreach material that match incumbents' public text, supporting the clean-room rule (CONSTITUTION §8) and REQ-OUTR-007's ban on copied text. It is separate from OUTR.BRAND.LINT because it needs a reference set of third-party text, and whether the project may hold one is a maintainer decision (D-013).

## Scope

**In:**
- A lint rule (proposed ID CL003) plugged into the OUTR.BRAND.LINT rule file
- The reference set, in whatever form D-013 allows, and how it is built and updated

**Out** (and which node owns it instead):
- The lint engine, sentence extraction and other rules (owned by `OUTR.BRAND.LINT`)
- The human "copied text" review item (owned by `OUTR.BRAND.REVIEW`)

## Requirements

- **REQ-OUTR.BRAND.COPY-001** When a source contains a run of 15 or more consecutive words that matches an entry of the reference set after case and whitespace normalisation, the lint shall report an error with rule ID CL003, file and line [blocked on D-013].
- **REQ-OUTR.BRAND.COPY-002** The reference set shall be stored and distributed only in the form allowed by D-013, and the project's public repository shall never contain incumbent text verbatim.
- **REQ-OUTR.BRAND.COPY-003** Rule CL003 shall have at least one passing and one failing fixture made of project-authored text only.

## Interfaces

- **Offers:** rule CL003 for the claim lint.
- **Consumes:** OUTR.BRAND.LINT rule-file format; D-013.

## Decomposition hints

After D-013: SET (reference set build), RULE (matcher), FIX (fixtures).

## Open questions

- **Unblocked (2026-11-17):** D-013 (Copy detection against incumbents' public text) is accepted. Apply its Decision section (`decisions/D-013-*.md`) when this node is expanded.
- Blocked on D-013: whether a reference set may be kept at all, and in what form. If D-013 chooses option C, this node is retired and REQ-OUTR.BRAND-005 is changed by the steering group.

## Children

_None yet: status `decision`._
