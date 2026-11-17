---
id: IND.PATTERN.TPL
title: Pattern report template
level: 3
parent: IND.PATTERN
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PATTERN-001]
covers: []
depends_on: [IND.PATTERN.SUM, RPT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IND.PATTERN.TPL: Pattern report template

## Purpose

The pattern report template: per-pattern summary (counts per outcome, worst member) with an expandable per-member table. Registered through the pack manifest using only public RPT template features.

## Scope

**In:**
- Template registration through the manifest
- Summary block and per-member table

**Out** (and which node owns it instead):
- Report engine (owned by `RPT`)
- Summary values (owned by `IND.PATTERN.SUM`)

## Requirements

- **REQ-IND.PATTERN.TPL-001** The pattern template shall be registered only through the pattern pack manifest and shall use only public RPT template features.
- **REQ-IND.PATTERN.TPL-002** The template shall show per pattern the counts per outcome, the worst member with its characteristic and the per-member table in member order.

## Interfaces

- **Offers:** report template `pack.pattern.summary`.
- **Consumes:** RPT template registration; `IND.PATTERN.SUM` output.

## Decomposition hints

Likely a single leaf once RPT's template format exists.

## Open questions

- None.

## Children

_None yet: status `open`._
