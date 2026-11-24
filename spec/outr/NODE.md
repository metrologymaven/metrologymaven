---
id: OUTR
title: Outreach: website, docs portal, launch and adoption
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-022, REQ-FID-034]
covers: []
depends_on: [ECO]
cli: []
mcp: []
journal: []
updated: 2026-11-22
---

# OUTR: Outreach

## Purpose

Everything that brings users and contributors to the project:
- the project website;
- the docs portal and book;
- download and status pages;
- community channels;
- launch and event plans;
- content;
- published benchmarks;
- privacy-respecting adoption metrics.

These are open-source deliverables that contributors can own. The **commercial** plan (pricing, sales, funding) is deliberately not here. It lives in the founder's business documents, and this node only links to them.

## Scope

**In:**
- Website: home, per-app pages, download, status and parity, roadmap, blog
- Docs portal: user guide, CLI/MCP/Python references generated from the registry, contributor guide, executable WASM-interactive Metrology Field Guide (D-054)
- Brand usage guidelines (after D-002)
- Community channels and moderation rules
- Launch plan: alpha → beta → 1.0 milestones and announcement channels
- Conference and trade-show demo kits
- Content: tutorials, migration guides from incumbents' workflows, videos produced with the built-in recorder
- Public benchmarks and case studies
- Adoption metrics without product telemetry (downloads, stars, forum activity, opt-in surveys)
- Instrument-vendor and university outreach programs
- The GD&T interpretation community: public rule pages, interpretation profiles, cases, reviewers and reference suites (D-045)

**Out:**
- Pricing, sales, funding (business documents, outside this tree)
- Release packaging (ECO)
- The parity report generator (ECO; OUTR publishes it)

## Requirements

- **REQ-OUTR-001** The project website shall present each focused app with its status, known gaps and the generated parity and coverage reports, and shall never claim a capability the parity report does not show as live.
- **REQ-OUTR-002** The docs portal shall generate CLI, MCP and Python reference pages from the registry manifest for each release, and shall host user, migration and contributor guides.
- **REQ-OUTR-003** The website and docs shall be static, self-hostable, use no third-party trackers, and meet the accessibility baseline set in VIZ.A11Y.
- **REQ-OUTR-004** The project shall maintain a written launch plan with milestone criteria (alpha, beta, 1.0) tied to the spec tree's phase gates.
- **REQ-OUTR-005** Every tutorial and migration guide shall be reproducible from a published example project and journal, and its videos shall be recorded with the platform's own capture commands.
- **REQ-OUTR-006** Adoption shall be measured without product telemetry, using only public signals and opt-in surveys, and reported quarterly.
- **REQ-OUTR-007** Marketing material shall follow the vocabulary rules in CONSTITUTION C9 and the vocabulary lint: no incumbent trade dress or copied text, no unqualified "certified" or "compliant" claims.
- **REQ-OUTR-008** The project shall run outreach programs for instrument vendors (driver SDK and conformance) and universities (teaching kits, datasets).
- **REQ-OUTR-009** The project shall run a public GD&T interpretation community (rule pages, interpretation profiles, a case pipeline, a public reviewer role and public reference suites) that never publishes standards text, gives every interpretation rule a decision owner and a deadline, and routes questions about specific customer parts to paid support (D-045).
- **REQ-OUTR-010** The project shall publish an executable, WASM-interactive Metrology Field Guide backed by implemented kernel crates and a transparent dimensional metrology field map (D-054).

## Interfaces

- **Offers:** the website, docs portal, launch plan, outreach program pages.
- **Consumes:** ECO (parity reports, releases, registry docs), VIZ capture (videos), the spec tree (status).

## Decomposition hints

SITE (website), DOCS (docs portal and book), BRAND, COMM (channels and moderation), LAUNCH (milestones, events, demo kits), CONTENT (tutorials, migration guides, videos), BENCH (public benchmarks, case studies), METRICS, PROG (vendor and university programs).

## Split rationale

The split is by **audience-facing deliverable**, with the shared rule set pulled out. Each child is one thing a contributor can own end to end (a site, a portal, a plan, a body of content, a program), and every child that publishes material runs the one claim lint owned by OUTR.BRAND. Honest status is enforced at two places: OUTR.SITE refuses capability claims the parity report does not show as live, and OUTR.BENCH backs every number with a reproducible journal.

Mapping from the decomposition hints: SITE, DOCS, BRAND, LAUNCH, CONTENT, BENCH and PROG are kept. COMM and METRICS are merged into OUTR.COMM, because public channel activity is the main input to the adoption metrics and both are governed by the same no-telemetry rule.

Alternatives considered:
- **Split by milestone** (alpha, beta, 1.0 outreach). Rejected: the same site, docs and content serve every milestone, so each child would repeat the others.
- **Split by audience** (users, contributors, vendors, universities). Rejected: the website and docs serve all audiences, and splitting them by audience duplicates the honest-status and claim checks.
- **Keep METRICS separate.** Rejected: it would have no requirement other than REQ-OUTR-006 to share with COMM, and two nodes owning public-signal collection would overlap.

## Open questions

- **D-045 (accepted 2026-11-19):** added REQ-OUTR-009 and child `OUTR.GDTC`. It is a separate child, not part of `OUTR.COMM` or `OUTR.BENCH`, because its pages publish the review state of interpretation rules and its suites are run by third parties against any software, which are rules neither sibling has.
- **D-054 (accepted 2026-11-22):** added REQ-OUTR-010 allocated to `OUTR.DOCS` (`OUTR.DOCS.GUIDE`) so the executable Metrology Field Guide shares the docs portal's offline bundle, runnable-example verification, and zero-third-party-origin gates.
- An existing site project may already exist in `../../metrology maven/site` [unverified relationship]. Decide whether OUTR.SITE reuses it (carried into OUTR.SITE).
- ECO's scope also lists "Docs site and book" and "Example corpora and benchmark datasets". This split assumes ECO owns the docs build, generators and datasets, and OUTR owns the portal, content and published results. Confirm when ECO is split.
- REQ-OUTR-007 refers to "the vocabulary lint"; OUTR.BRAND owns the outreach claim lint. No node yet owns a product UI-string lint.
- Brand guidelines in OUTR.BRAND are blocked by D-002.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.SITE](site/NODE.md) | Project website | 001 003 | none |
| [OUTR.DOCS](docs/NODE.md) | Docs portal and book | 002 003 010 | none |
| [OUTR.BRAND](brand/NODE.md) | Brand, vocabulary and claim rules | 007 | none |
| [OUTR.COMM](comm/NODE.md) | Community channels and adoption metrics | 006 | none |
| [OUTR.LAUNCH](launch/NODE.md) | Launch plan, milestones and event kits | 004 | none |
| [OUTR.CONTENT](content/NODE.md) | Tutorials, migration guides and videos | 005 | none |
| [OUTR.BENCH](bench/NODE.md) | Public benchmarks and case studies | 001 007 | none |
| [OUTR.PROG](prog/NODE.md) | Instrument-vendor and university programs | 008 | none |
| [OUTR.GDTC](gdtc/NODE.md) | GD&T interpretation community | 009 | none |
