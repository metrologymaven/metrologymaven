---
id: OUTR.SITE.PAGES
title: Home page, per-app pages and blog
level: 3
parent: OUTR.SITE
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-001]
covers: []
depends_on: [OUTR.SITE.GEN, ECO.PARITY, ECO.APPS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.SITE.PAGES: Home page, per-app pages and blog

## Purpose

The editorial pages of the site: the home page, one page per focused app of the D-004 family, and the blog (release notes and project news). Each app page combines short hand-written text with generated status and known-gaps data, so the honest-status rule holds even on the most promotional pages.

## Scope

**In:**
- Home page
- One page per focused app (Inspect, Probe, Network, Report, and any app later added by decision)
- Generated phase, status and known-gaps blocks on each app page
- Blog with release notes and news, and a feed file

**Out** (and which node owns it instead):
- Status, parity and roadmap pages (owned by `OUTR.SITE.STATUS`)
- Download page (owned by `OUTR.SITE.DL`)
- The capability-tag check on these pages (owned by `OUTR.SITE.CLAIM`)
- Tutorials and benchmark pages (owned by `OUTR.CONTENT` and `OUTR.BENCH`)
- Program pages (owned by `OUTR.PROG`)

## Requirements

- **REQ-OUTR.SITE.PAGES-001** The site shall have exactly one page for each focused app declared in the ECO.APPS manifest, and the build shall fail if an app has no page or a page names no declared app.
- **REQ-OUTR.SITE.PAGES-002** Each app page shall show the app's phase and status (pre-alpha, alpha, beta or stable) taken from the ECO.PARITY status report of the published release, not from hand-written text.
- **REQ-OUTR.SITE.PAGES-003** Each app page shall show the app's known-gaps list from the ECO.PARITY status report, with a count of live against in-scope seed rows and a link to the full parity page.
- **REQ-OUTR.SITE.PAGES-004** Every blog post shall show its publication date and, for release notes, the release tag it describes and a link to that release's download entry.
- **REQ-OUTR.SITE.PAGES-005** The blog shall publish a static feed file (Atom or RSS) listing every post, generated at build time.

## Interfaces

- **Offers:** home, app and blog pages; capability tags in page sources for OUTR.SITE.CLAIM to check; the blog feed.
- **Consumes:** OUTR.SITE.GEN template and imported `status.json`; ECO.APPS app manifest; OUTR.SITE.DL anchors for release links.

## Decomposition hints

HOME, APP (per-app page template with generated blocks), BLOG (posts and feed).

## Open questions

- Whether an app with status pre-alpha gets a full page or a stub is an editorial choice for maintainers.

## Children

_None yet: status `open`._
