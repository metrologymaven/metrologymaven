---
id: OUTR.DOCS
title: Docs portal and book
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-002, REQ-OUTR-003]
covers: []
depends_on: [ECO, AUTO, CORE, OUTR.BRAND, VIZ]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS: Docs portal and book

## Purpose

The versioned documentation portal: generated CLI, MCP and Python reference pages for every release, the user guide (book), the contributor guide, and the hosting frame for tutorials and migration guides authored in OUTR.CONTENT. It is separate from the website because it is versioned per release and mostly generated from the registry manifest (CONSTITUTION C3), while the website is a single current edition.

## Scope

**In:**
- Reference pages for CLI commands, MCP tools and Python bindings, generated from the registry manifest of each release
- Per-release versioning, a version switcher, and a "latest" alias
- User guide (book): concepts, per-app workflows, units, journals, replay, verification
- Contributor guide: spec-tree workflow, claiming nodes, house rules, building from source
- Hosting, navigation and search for guides authored by OUTR.CONTENT
- Static build, offline bundle, no trackers, accessibility conformance

**Out** (and which node owns it instead):
- The registry manifest and the reference generators themselves (owned by `CORE` and `AUTO`; ECO owns the docs build in CI)
- Tutorials, migration guides and videos (owned by `OUTR.CONTENT`)
- Project website pages (owned by `OUTR.SITE`)
- Governance policy text (GOVERNANCE.md, decided by D-003)
- Vocabulary and claim lint rules (owned by `OUTR.BRAND`)

## Requirements

- **REQ-OUTR.DOCS-001** For each release, the portal shall contain exactly one CLI page, one MCP page and one Python page for every command in that release's registry manifest, and no reference page for a command that is not in it.
- **REQ-OUTR.DOCS-002** If any reference page differs from what the generator produces from the release's registry manifest, then the docs build shall fail and name the page.
- **REQ-OUTR.DOCS-003** Each reference page shall show the command's ID, side-effect class, determinism class and MCP projection as declared in the manifest, and shall state when a command is not exposed on MCP.
- **REQ-OUTR.DOCS-004** The portal shall keep a separate, browsable edition for every release still in support under the ECO release policy, and every page shall show the release it documents.
- **REQ-OUTR.DOCS-005** The user guide shall have at least one chapter per focused app defined by D-004 and one chapter each on journals and replay, units, and result verification.
- **REQ-OUTR.DOCS-006** The contributor guide shall explain how to find, claim, expand and validate a spec node, and every command it shows shall run unchanged against the repository revision it documents.
- **REQ-OUTR.DOCS-007** The portal shall build to static files that can be downloaded as one offline bundle per release and browsed without network access.
- **REQ-OUTR.DOCS-008** When a docs page is loaded, it shall make zero requests to origins other than its own, and site search shall run entirely client-side.
- **REQ-OUTR.DOCS-009** Every docs page shall pass the automated checks of the VIZ accessibility baseline with zero violations at the required level.
- **REQ-OUTR.DOCS-010** The docs build shall fail on any broken internal link, and shall report external links that fail a check run at least weekly [unverified cadence].

## Interfaces

- **Offers:** the docs portal; per-release offline bundles; a page frame and navigation slots for OUTR.CONTENT guides; stable URLs for each command reference page (used by OUTR.SITE and error messages).
- **Consumes:** registry manifest (CORE), reference generators (AUTO), docs CI and release metadata (ECO), OUTR.BRAND lint rules and assets, VIZ accessibility baseline.
- No registry commands of its own.

## Decomposition hints

REF (reference page generation and drift check), VER (versioning and offline bundles), BOOK (user guide), CONTRIB (contributor guide), SEARCH, QA (links, a11y, trackers).

## Open questions

- ECO's scope lists "Docs site and book". This split assigns ECO the docs **build in CI and the generators**, and OUTR.DOCS the **portal structure, content and publishing**. If ECO's expansion claims the portal itself, the overlap must be resolved by a human reviewer.
- How long a release stays "in support" is ECO's call; REQ-OUTR.DOCS-004 follows whatever ECO decides.

## Split rationale

The split follows the **kind of page and its source of truth**, with the shared frame and the gates pulled out. REF is pure registry output with a drift check; BOOK and CONTRIB are authored text with executable examples; FRAME holds editions, navigation, guide slots and offline bundles for all of them; SEARCH and QA are cross-cutting services. QA reuses the OUTR.SITE.QA gates instead of building a second crawler.

Alternatives considered:
- **Split by release lifecycle** (build, publish, archive). Rejected: every stage touches every page type, so ownership would blur.
- **Merge SEARCH into FRAME.** Rejected: search has its own offline and no-third-party constraints and a separate index budget, and is a natural unit for one contributor.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.DOCS.REF](ref/NODE.md) | Generated command reference pages | 001 002 003 | none |
| [OUTR.DOCS.FRAME](frame/NODE.md) | Portal frame, release editions and offline bundles | 004 007 | none |
| [OUTR.DOCS.BOOK](book/NODE.md) | User guide (the book) | 005 | none |
| [OUTR.DOCS.CONTRIB](contrib/NODE.md) | Contributor guide with checked commands (leaf) | 006 | none |
| [OUTR.DOCS.SEARCH](search/NODE.md) | Client-side docs search | 008 | none |
| [OUTR.DOCS.QA](qa/NODE.md) | Docs quality gates and link checks | 008 009 010 | none |
