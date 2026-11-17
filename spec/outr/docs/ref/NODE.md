---
id: OUTR.DOCS.REF
title: Generated command reference pages
level: 3
parent: OUTR.DOCS
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.DOCS-001, REQ-OUTR.DOCS-002, REQ-OUTR.DOCS-003]
covers: []
depends_on: [CORE.REG, AUTO.CLI, AUTO.MCP, AUTO.PY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.DOCS.REF: Generated command reference pages

## Purpose

The CLI, MCP and Python reference pages of each release, placed into the portal from the output of the registry-driven generators, with a drift check that fails the docs build if any page differs from what the generators produce. It is separate because it is the part of the portal that must be exactly the registry (CONSTITUTION C3), with no hand-edited text.

## Scope

**In:**
- Placing generated CLI, MCP and Python pages for every command of a release's registry manifest into the portal
- The completeness check (one page of each kind per command, none extra)
- The drift check against fresh generator output
- The fields each page must show (ID, side-effect class, determinism class, MCP projection)
- Stable URLs per command and release

**Out** (and which node owns it instead):
- The registry manifest (owned by `CORE.REG`)
- The page generators (owned by `AUTO.CLI`, `AUTO.MCP`, `AUTO.PY`; ECO runs them in CI)
- Versions, navigation and offline bundles (owned by `OUTR.DOCS.FRAME`)
- Search index (owned by `OUTR.DOCS.SEARCH`)

## Requirements

- **REQ-OUTR.DOCS.REF-001** For each release, the portal shall contain one CLI page, one MCP page and one Python page for every command in that release's registry manifest, and the docs build shall fail if any is missing.
- **REQ-OUTR.DOCS.REF-002** If the portal contains a reference page for a command ID that is not in the release's registry manifest, then the docs build shall fail and name the page.
- **REQ-OUTR.DOCS.REF-003** If a reference page differs byte-for-byte from the output of the release's generators run on its registry manifest, then the docs build shall fail and name the page.
- **REQ-OUTR.DOCS.REF-004** Each reference page shall show the command ID, side-effect class, determinism class and MCP projection as declared in the manifest.
- **REQ-OUTR.DOCS.REF-005** When a command's side-effect class is not projected to MCP (M1, A0, S1, or human-only W3), its MCP page shall state that the command is not available on MCP and why, instead of a tool schema.
- **REQ-OUTR.DOCS.REF-006** Each reference page shall have a URL derived only from the release and the command ID, so links in error messages and on the website stay valid for that release.

## Interfaces

- **Offers:** per-release reference pages with stable URLs `/<release>/ref/<cli|mcp|python>/<command-id>`; the completeness and drift checks.
- **Consumes:** CORE.REG manifest; AUTO.CLI, AUTO.MCP and AUTO.PY generator output; OUTR.DOCS.FRAME page frame.

## Decomposition hints

PLACE (placing generated pages and URL scheme), COMPLETE (completeness check), DRIFT (byte comparison with fresh output).

## Open questions

- Whether the generators emit finished pages or structured data that the portal renders is for AUTO to decide; the drift check works either way if the rendering is deterministic.

## Children

_None yet: status `open`._
