---
id: AUTO.SURF.DOCS
title: Generated command reference pages
level: 3
parent: AUTO.SURF
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.SURF-001, REQ-AUTO.SURF-002]
covers: []
depends_on: [AUTO.SURF.GEN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.DOCS: Generated command reference pages

## Purpose

The docs emitter: one reference page per command major, generated from the manifest (summary, parameters with units, result type, side-effect class and what it means on each surface, determinism class, CLI usage, MCP tool name or "not on MCP", Python signature, examples declared in the registry). It is also the single source of the help text that `AUTO.CLI.TREE` prints, so `--help` and the docs cannot disagree.

## Scope

**In:**
- Reference-page emitter (`SurfaceEmitter` for surface `docs`)
- Help-text model shared with the CLI emitter
- Per-domain index pages and a class legend
- Example blocks taken only from registry-declared examples

**Out:**
- Publishing and site navigation (`OUTR.DOCS`)
- Tutorials, how-tos and Skills (`AUTO.MCP.SKILLS`, `OUTR`)
- CLI rendering of help (`AUTO.CLI.TREE`)

## Requirements

- **REQ-AUTO.SURF.DOCS-001** The emitter shall write exactly one reference page per command major in the manifest and list it in the docs `surface-index.json`.
- **REQ-AUTO.SURF.DOCS-002** Each page shall state the command's side-effect class and, for each surface, whether and how the command is reachable (for example "request only on MCP", "not on MCP").
- **REQ-AUTO.SURF.DOCS-003** The emitter shall build page text and CLI help text from one `HelpText` value per command, so the two cannot diverge for the same manifest digest.
- **REQ-AUTO.SURF.DOCS-004** Pages shall contain only text taken from the registry declaration and fixed templates, and shall contain no standards text.
- **REQ-AUTO.SURF.DOCS-005** Each page shall name its manifest digest and the command definition's own digest.

## Interfaces

- **Offers:** docs emitter; `HelpText` model consumed by `AUTO.CLI.TREE`; Markdown pages consumed by `OUTR.DOCS`.
- **Consumes:** `AUTO.SURF.PROJ` projection; `AUTO.SURF.GEN` framework.

## Decomposition hints

PAGE (per-command page template), HELP (shared help-text model), INDEX (domain indexes and class legend). Could become a single M leaf once `OUTR.DOCS` fixes the page format (Markdown flavour, front matter, link scheme).

## Open questions

- Page format and link scheme depend on the docs portal (`OUTR.DOCS`, open). Kept open until that is fixed.

## Children

_None yet: status `open`._
