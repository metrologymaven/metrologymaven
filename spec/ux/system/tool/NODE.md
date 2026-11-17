---
id: UX.SYSTEM.TOOL
title: Design tool, design-file storage and contribution workflow
level: 3
parent: UX.SYSTEM
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.SYSTEM-006, REQ-UX.SYSTEM-007]
covers: []
depends_on: [UX.SYSTEM.PKG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.SYSTEM.TOOL: Design tool, design-file storage and contribution workflow

## Purpose

Where design source files live, which tool the project recommends, and how anyone proposes a token, icon, component, pattern or screen. The repository text files stay the single source of truth; the design tool is a convenience whose files are exported into the repository in open formats. A small check verifies that every design source a UX node refers to is retrievable without a paid account and has an open-format export committed.

## Scope

**In:** the tool recommendation and its rationale; `design/sources.json` (`design-sources@1`); `design/CONTRIBUTING.md` (design contribution workflow); `fid-design sources-check`.
**Out:** the review gate (`UX.SYSTEM.REVIEW`); package format (`UX.SYSTEM.PKG`); hosting infrastructure for a self-hosted design server (ECO, if ever needed).

## Requirements

- **REQ-UX.SYSTEM.TOOL-001** The recommended design tool shall be Penpot (open source, self-hostable, usable without a paid account), and token values shall be authored in the repository's token files, not only in the tool.
- **REQ-UX.SYSTEM.TOOL-002** Every design source listed in `sources.json` shall name its tool, a public location (repository path or public share URL) and at least one committed export in an open format (SVG, PDF or PNG, plus the tool's own export file where the tool offers one).
- **REQ-UX.SYSTEM.TOOL-003** If a listed source has no committed export, or an export's SHA-256 differs from the digest recorded in `sources.json`, then `fid-design sources-check` shall fail and name the source.
- **REQ-UX.SYSTEM.TOOL-004** If a source names a tool marked `requires_paid_account = true` in the tool table, then `sources-check` shall fail with `SOURCE-PAID-TOOL`.
- **REQ-UX.SYSTEM.TOOL-005** The contribution guide shall describe, for each artefact kind, how to propose it (issue template, files to add, originality declaration, review record) so that a contributor with no design-tool account can contribute by editing text and SVG files.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/system/tool/ok/` (two sources with committed SVG and PDF exports and matching digests), when `fid-design sources-check --root ok --json` runs, then it exits 0.
- **AC-2** (error path) Given `no-export/`, when the check runs, then it exits 1 with `SOURCE-EXPORT-MISSING <id>`.
- **AC-3** (error path) Given `stale-export/` (export digest differs), when the check runs, then it exits 1 with `SOURCE-EXPORT-DIGEST <id>`.
- **AC-4** (error path) Given `paid-tool/` (a source whose tool row has `requires_paid_account = true`), when the check runs, then it exits 1 with `SOURCE-PAID-TOOL <id>`.

## Design

- **Recommendation (Penpot over Figma with exported tokens):** Penpot is open source and self-hostable, so REQ-UX.SYSTEM-006 holds even if a hosted service changes terms [unverified current Penpot licence and export formats; check at adoption]. Figma's free tier would work only with every token and export mirrored into the repository, and a contributor would still need a Figma account to edit the source. Using either tool does not change the project's own licences (D-001), so no decision record is needed; maintainers adopt the recommendation in review of this leaf.
- **Tool table** in `sources.json`: `tools: [{ id, name, open_source, requires_paid_account }]`; `sources: [{ id, node, tool, location, exports: [{ path, sha256 }] }]`.
- **Command:** `fid-design sources-check --root <design dir> [--json]`. Tooling, nearest class R0. It checks committed files only; it never fetches the public URL.
- **Errors:** `SOURCE-EXPORT-MISSING`, `SOURCE-EXPORT-DIGEST`, `SOURCE-PAID-TOOL`, `SOURCE-NODE-UNKNOWN`, `SOURCES-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `sources`.

## Test fixtures

`fixtures/ux/system/tool/`: `ok/`, `no-export/`, `stale-export/`, `paid-tool/`; tiny synthetic SVG and PDF files, CC-BY-4.0.

## Build prompt

> Write `design/CONTRIBUTING.md`: the repository text files are the source of truth; Penpot is the recommended design tool; how to propose a token, icon, component, pattern, screen or report style (issue template, files to add, originality declaration, design-review record), and how to contribute with only a text editor and SVG files. Create `design/sources.json` (`design-sources@1`: tools table with open_source and requires_paid_account; sources with node ID, tool, public location, exports with SHA-256).
>
> In the Rust tooling crate `fid-design`, add `fid-design sources-check --root <dir> [--json]`: every source has at least one committed open-format export (svg, pdf, png, or the tool's export file) whose SHA-256 matches; fail on a paid-account tool; never access the network. Errors `SOURCE-EXPORT-MISSING|SOURCE-EXPORT-DIGEST|SOURCE-PAID-TOOL|SOURCE-NODE-UNKNOWN|SOURCES-SCHEMA`. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.SYSTEM.TOOL.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded reads; no network.

## Open questions

- If maintainers prefer Figma, REQ-UX.SYSTEM.TOOL-001 changes and every source must still meet REQ-002 and REQ-004 (which would then fail for Figma unless its free tier is marked as not requiring a paid account) [unverified Figma free-tier terms].

## Children

_Leaf: none._
