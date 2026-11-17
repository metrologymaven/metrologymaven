---
id: OUTR.DOCS.QA
title: Docs quality gates and link checks
level: 3
parent: OUTR.DOCS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-008, REQ-OUTR.DOCS-009, REQ-OUTR.DOCS-010]
covers: []
depends_on: [OUTR.DOCS.FRAME, OUTR.SITE.QA]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS.QA: Docs quality gates and link checks

## Purpose

The checks every docs edition must pass before publication: zero off-origin requests and the accessibility baseline (by running the reusable OUTR.SITE.QA gates with docs configuration), plus the docs-specific internal and external link checks. It is separate from SITE.QA only in configuration and in the link checks, which matter most for a large, versioned portal.

## Scope

**In:**
- Docs configuration of the OUTR.SITE.QA gates (origin, page list, budgets) for every edition
- Internal link check (fails the build)
- Scheduled external link check (reports)

**Out** (and which node owns it instead):
- The gate implementations (owned by `OUTR.SITE.QA`)
- Search behaviour (owned by `OUTR.DOCS.SEARCH`)
- Reference drift check (owned by `OUTR.DOCS.REF`)

## Requirements

- **REQ-OUTR.DOCS.QA-001** When an edition is built, the OUTR.SITE.QA off-origin crawl shall run on every page of the edition and fail the docs build on any off-origin request.
- **REQ-OUTR.DOCS.QA-002** When an edition is built, the OUTR.SITE.QA accessibility gate shall run on every page and fail the docs build on any violation at the VIZ baseline's required level.
- **REQ-OUTR.DOCS.QA-003** If any internal link or anchor in an edition does not resolve to a page or anchor of the same build, then the docs build shall fail and name the source page and the link.
- **REQ-OUTR.DOCS.QA-004** An external link check shall run at least weekly [unverified cadence] on the latest edition and open or update one tracking issue listing every failing external link.
- **REQ-OUTR.DOCS.QA-005** When an edition is built, the OUTR.BRAND claim lint shall run on its hand-written sources (not on generated reference pages) and fail the build on any error.

## Interfaces

- **Offers:** a docs gate configuration and the link-check reports.
- **Consumes:** OUTR.SITE.QA gates; OUTR.BRAND.LINT; built editions from OUTR.DOCS.FRAME.

## Decomposition hints

GATES (configuration of shared gates), LINKS (internal and external link checks).

## Open questions

- The weekly external-link cadence is a proposal carried from OUTR.DOCS.

## Children

_None yet: status `open`._
