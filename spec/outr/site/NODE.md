---
id: OUTR.SITE
title: Project website
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-001, REQ-OUTR-003]
covers: []
depends_on: [ECO, OUTR.BRAND, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.SITE: Project website

## Purpose

The public, static, self-hostable project website: home page, one page per focused app, download page, status and parity page, roadmap and blog. It exists as a separate unit because it is the one place where the project makes claims to the public, so it carries the **honest-status** rule: every capability claim is generated from, or checked against, the ECO parity report and the spec tree, never written by hand (pattern 12 and 14 in `research/artcraft-design-lessons.md`).

## Scope

**In:**
- Home page and per-app pages (one per app in the D-004 family), each with status, known gaps and links to the generated parity and coverage reports
- Download page that links to ECO release artifacts and checksums
- Status and parity page that renders the ECO parity report and `generated/COVERAGE.md` / `generated/INDEX.md` without editing them
- Roadmap page generated from spec-tree phase tags
- Blog (release notes, project news)
- Static build, self-hosting instructions, no third-party trackers, accessibility conformance
- A build-time claim check that blocks publication of unsupported capability claims

**Out** (and which node owns it instead):
- Docs portal, reference pages and guides (owned by `OUTR.DOCS`)
- Brand assets, vocabulary rules and the marketing claim lint rules (owned by `OUTR.BRAND`; SITE runs the lint)
- Tutorials, migration guides and videos (owned by `OUTR.CONTENT`)
- Benchmark and case-study content (owned by `OUTR.BENCH`; SITE hosts it)
- Launch milestones and announcements (owned by `OUTR.LAUNCH`)
- Program pages' content for vendors and universities (owned by `OUTR.PROG`)
- Parity report generation and release packaging (owned by `ECO`)
- Pricing, sales and funding pages (business documents outside this tree; the site may at most link to them)

## Requirements

- **REQ-OUTR.SITE-001** The website shall have one page per focused app defined by D-004, and each page shall show the app's phase, status (pre-alpha, alpha, beta or stable), and a known-gaps list generated from the spec tree and the ECO parity report.
- **REQ-OUTR.SITE-002** The status and parity page shall render the ECO parity report and the spec-tree coverage report of the matching release byte-for-byte from their generated sources, with the source revision and generation date shown.
- **REQ-OUTR.SITE-003** If a page asserts a capability (by a capability tag in the page source) that the parity report for the published release does not show as live, then the site build shall fail and name the page and the capability.
- **REQ-OUTR.SITE-004** The roadmap page shall be generated from the `phase:` and `status:` fields of the spec tree, and shall show no dates that are not recorded in a launch-plan milestone owned by OUTR.LAUNCH.
- **REQ-OUTR.SITE-005** The download page shall list, for each release, every installer and web build published by ECO with its SHA-256 checksum and signature link, and shall link only to project-controlled hosts.
- **REQ-OUTR.SITE-006** The website shall build to static files with no server-side code, and a documented single-command build shall reproduce it from the repository at a given revision.
- **REQ-OUTR.SITE-007** When a built page is loaded, it shall make zero requests to origins other than the site's own origin (no third-party fonts, scripts, analytics or embeds), verified by an automated crawl of every built page.
- **REQ-OUTR.SITE-008** Every built page shall pass the automated checks of the accessibility baseline defined by VIZ (VIZ.A11Y once it exists), with zero violations at the baseline's required level.
- **REQ-OUTR.SITE-009** Before publication, every page and blog post shall pass the marketing vocabulary and claim lint defined by OUTR.BRAND, with zero errors.
- **REQ-OUTR.SITE-010** Each page's transferred size without media shall stay under a budget recorded in the site repository (initial target 500 KB [unverified]), and the build shall fail when a page exceeds it.

## Interfaces

- **Offers:** the public website; a capability-tag convention for page sources; a self-hosting guide.
- **Consumes:** ECO parity report, release manifest and checksums; spec-tree `generated/` reports; OUTR.BRAND assets and lint rules; OUTR.DOCS (links); OUTR.CONTENT, OUTR.BENCH, OUTR.LAUNCH and OUTR.PROG pages; VIZ accessibility baseline.
- No registry commands: the site is a project deliverable, not a product feature.

## Decomposition hints

GEN (static build and data import from generated reports), APPS (per-app and status pages), CLAIM (capability-tag check), DL (download page), BLOG, QA (tracker crawl, a11y and size checks).

## Open questions

- An existing site project may already exist in `../../metrology maven/site` [unverified relationship]. Decide whether OUTR.SITE reuses it (carried over from OUTR).
- The static site generator choice is an implementation choice for the leaf; the constitution's Rust-first rule targets the product, and whether it binds project tooling is not stated.
- OUTR.SITE.CLAIM assumes `parity.json` exposes `release` and `rows[]` with `seed_id` and `live`; ECO.PARITY must confirm when it is expanded.

## Split rationale

The split follows the **page families and their data sources**, with the shared build and the shared quality gates pulled out. GEN owns the reproducible static build and the import of generated inputs; PAGES, STATUS and DL each render one kind of page from one kind of generated source (app status, parity and coverage reports, release manifest); CLAIM is the honest-status check on hand-written pages; QA runs the gates every page must pass and is reusable by OUTR.DOCS.

Alternatives considered:
- **One child per page** (home, each app, blog, status, roadmap, download). Rejected: too many tiny nodes sharing the same data import and checks.
- **Content vs tooling** (all pages in one child, all checks in another). Rejected: the checks are of different kinds (claim, size, trackers, accessibility), and the per-release download and status pages need their own generation logic.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.SITE.GEN](gen/NODE.md) | Static build and generated-data import | 006 | none |
| [OUTR.SITE.PAGES](pages/NODE.md) | Home page, per-app pages and blog | 001 | none |
| [OUTR.SITE.STATUS](status/NODE.md) | Status, parity and roadmap pages | 002 004 | none |
| [OUTR.SITE.CLAIM](claim/NODE.md) | Capability-claim check (leaf) | 003 | none |
| [OUTR.SITE.DL](dl/NODE.md) | Download page | 005 | none |
| [OUTR.SITE.QA](qa/NODE.md) | Static-site quality gates | 007 008 009 010 | none |
