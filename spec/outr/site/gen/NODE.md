---
id: OUTR.SITE.GEN
title: Static build and generated-data import
level: 3
parent: OUTR.SITE
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.SITE-006]
covers: []
depends_on: []
uses_artifacts: [ECO.PARITY, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.SITE.GEN: Static build and generated-data import

## Purpose

The build pipeline that turns the site repository at a given revision into a folder of static files, and the import step that pulls generated inputs (ECO parity and status reports, the release manifest, spec-tree `generated/` reports) into the build as read-only data. Every other SITE child renders pages on top of this pipeline, so it is the single place where reproducibility and self-hosting are guaranteed.

## Scope

**In:**
- The single-command build and its pinned toolchain
- Import of generated inputs by revision or release tag, with digests recorded
- Self-hosting instructions (serve the output folder from any static web server)
- Page template and layout shared by all site pages, with OUTR.BRAND assets

**Out** (and which node owns it instead):
- Page content for apps, blog, status, roadmap and downloads (owned by `OUTR.SITE.PAGES`, `OUTR.SITE.STATUS`, `OUTR.SITE.DL`)
- The capability-claim check (owned by `OUTR.SITE.CLAIM`)
- Tracker, accessibility, lint and size gates (owned by `OUTR.SITE.QA`)
- Generating the reports themselves (owned by `ECO.PARITY`)

## Requirements

- **REQ-OUTR.SITE.GEN-001** The site shall build with one documented command from a clean checkout at a given revision, with every tool version pinned in the repository.
- **REQ-OUTR.SITE.GEN-002** When built twice from the same revision and the same imported inputs, the build shall produce byte-identical output files [unverified for every candidate generator].
- **REQ-OUTR.SITE.GEN-003** The build output shall consist only of static files (HTML, CSS, images, fonts, media and data files) and shall need no server-side code to serve.
- **REQ-OUTR.SITE.GEN-004** The build shall record, in a manifest file in its output, the site revision and the SHA-256 digest and source release of every imported generated input.
- **REQ-OUTR.SITE.GEN-005** If an imported generated input is missing or its digest does not match the release it claims to belong to, then the build shall fail and name the input.
- **REQ-OUTR.SITE.GEN-006** The site repository shall contain self-hosting instructions that let a third party serve the built output from a generic static web server without changes to the files.

## Interfaces

- **Offers:** the build command; the build manifest; the shared page template; an import API (data files at fixed paths) for the other SITE children.
- **Consumes:** ECO.PARITY `parity.json` and `status.json` per release, ECO.REL release manifest, spec-tree `generated/` reports, OUTR.BRAND assets.

## Decomposition hints

BUILD (pinned generator and command), IMPORT (fetch and digest check of generated inputs), HOST (self-hosting guide and template).

## Open questions

- Resolved in OUTR.SITE.GEN.BUILD: the generator is written in Rust inside the `fid-outreach` tooling crate; this is a choice, since CONSTITUTION §2 does not say whether the Rust-first rule binds tooling.
- Resolved in OUTR.SITE.GEN.BUILD: the existing site project in `../../metrology maven/site` was evaluated and is not reused (separate consultancy brand, third-party font requests, no generator or data import). A maintainer may overrule in review.
- Inputs are vendored and pinned (OUTR.SITE.GEN.IMPORT) rather than downloaded at build time, so the build is offline and reproducible; a new release's reports arrive through a reviewed pull request made by `site import fetch`.
- Per CONSTITUTION §2.1 the former `depends_on: [ECO.PARITY, ECO.REL]` is now `uses_artifacts`: SITE reads their published files only.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.SITE.GEN.BUILD](build/NODE.md) | Single-command static site build and shared page template (leaf) | 001 002 003 | none |
| [OUTR.SITE.GEN.IMPORT](import/NODE.md) | Generated-input lock file, fetch, digest check and build manifest (leaf) | 004 005 | none |
| [OUTR.SITE.GEN.HOST](host/NODE.md) | Self-hosting guide and generic static-server test (leaf) | 006 | none |