---
id: OUTR.SITE.QA
title: Static-site quality gates
level: 3
parent: OUTR.SITE
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-007, REQ-OUTR.SITE-008, REQ-OUTR.SITE-009, REQ-OUTR.SITE-010]
covers: []
depends_on: [OUTR.SITE.GEN, OUTR.BRAND.LINT, VIZ.SHELL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.SITE.QA: Static-site quality gates

## Purpose

The checks that run on every built page before the site is published: zero third-party requests, the accessibility baseline, the OUTR.BRAND claim lint and the page-size budget. It is built as a reusable static-site checker so that OUTR.DOCS runs the same gates on the docs portal instead of writing its own.

## Scope

**In:**
- A crawler that loads every built page and records every network request it makes
- Automated accessibility checks against the VIZ baseline
- Running the OUTR.BRAND claim lint on page and post sources
- The page-size budget and its check
- Packaging these gates so another static site (the docs portal) can run them with its own configuration

**Out** (and which node owns it instead):
- The lint rules (owned by `OUTR.BRAND.LINT`)
- The accessibility baseline itself (owned by `VIZ.SHELL`)
- Docs-specific link checks and search (owned by `OUTR.DOCS.QA` and `OUTR.DOCS.SEARCH`)
- The capability-claim check (owned by `OUTR.SITE.CLAIM`)

## Requirements

- **REQ-OUTR.SITE.QA-001** When the gates run, the crawler shall load every HTML page in the build output in a headless browser and fail if any page requests a resource from an origin other than the site's own.
- **REQ-OUTR.SITE.QA-002** The crawler's report shall list, for each failing page, every off-origin URL it requested and the element or script that caused the request where known.
- **REQ-OUTR.SITE.QA-003** The accessibility gate shall run the automated checks of the VIZ accessibility baseline on every built page and fail on any violation at the baseline's required level.
- **REQ-OUTR.SITE.QA-004** The lint gate shall run the OUTR.BRAND claim lint on every page and post source and fail on any error.
- **REQ-OUTR.SITE.QA-005** The size gate shall measure each page's transferred size without media and fail when a page exceeds the budget recorded in the repository (initial 500 KB [unverified]).
- **REQ-OUTR.SITE.QA-006** The gates shall take the site root, origin, size budget and page list as configuration, so that the same gates run unchanged on the docs portal.

## Interfaces

- **Offers:** one command that runs all four gates on a built static site and writes a machine-readable report; configuration format for other sites.
- **Consumes:** OUTR.SITE.GEN build output; OUTR.BRAND.LINT; VIZ.SHELL accessibility baseline (level and rule set).

## Decomposition hints

CRAWL (off-origin request check), A11Y (accessibility gate), SIZE (size budget), RUN (one entry point and report).

## Open questions

- VIZ has no VIZ.A11Y node; accessibility sits inside VIZ.SHELL. The required conformance level (for example WCAG 2.2 AA [unverified]) is VIZ's to state.
- The size budget of 500 KB is a proposal for maintainers to confirm.

## Children

_None yet: status `open`._
