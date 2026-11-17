---
id: OUTR.CONTENT.MIG
title: Migration guides from incumbent workflows
level: 3
parent: OUTR.CONTENT
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.CONTENT-001, REQ-OUTR.CONTENT-006, REQ-OUTR.CONTENT-007]
covers: []
depends_on: [OUTR.CONTENT.PUB, ECO.PARITY]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.CONTENT.MIG: Migration guides from incumbent workflows

## Purpose

Guides that walk a user of an incumbent product through doing the same job with ours, workflow by workflow, using the seed inventories as the checklist. Every workflow that has no equivalent yet is shown as a gap linked to its spec node. Written entirely in our own words (CONSTITUTION C9 and §8).

## Scope

**In:**
- One migration guide per seed inventory (GOM Inspect / ZEISS INSPECT, PolyWorks, PC-DMIS, SpatialAnalyzer, and any later inventory)
- A generated mapping table per guide: seed row to our command(s) or gap with spec node
- Prose that explains each workflow in our own words, with our screenshots only
- The example project and journal behind each guide

**Out** (and which node owns it instead):
- The parity data the mapping uses (owned by `ECO.PARITY`)
- Program conversion tools from incumbent formats (owned by other domains, for example `AUTO` and `IO`)
- Benchmarks or comparisons of results (owned by `OUTR.BENCH`)

## Requirements

- **REQ-OUTR.CONTENT.MIG-001** Each migration guide shall include a mapping table generated from the matching seed inventory and the release's parity data, with one row per in-scope seed row of that inventory.
- **REQ-OUTR.CONTENT.MIG-002** Each mapping row shall name the command(s) of ours that implement the workflow, or mark it as a gap with the spec node ID that will cover it.
- **REQ-OUTR.CONTENT.MIG-003** If a mapping table differs from a fresh generation from the same seed inventory and parity data, then the content build shall fail and name the guide.
- **REQ-OUTR.CONTENT.MIG-004** Migration guides shall contain no text, screenshots, icons or menu graphics copied from incumbent software or documentation, and shall use incumbent product names only to identify the products.
- **REQ-OUTR.CONTENT.MIG-005** Each guide shall name its example project and journal and the release it was last verified against, in the content manifest defined by OUTR.CONTENT.PUB.

## Interfaces

- **Offers:** migration guides (registered in OUTR.DOCS.FRAME); per-guide mapping tables.
- **Consumes:** `seeds/*.md` inventories; ECO.PARITY traceability data; OUTR.CONTENT.PUB checks; OUTR.BRAND.REVIEW for the no-copy review.

## Decomposition hints

MAP (mapping table generator), one child per incumbent guide (GOM, PWK, PCD, SA).

## Open questions

- Seed rows are all `[unverified]`; guides should say so until rows are confirmed by users of each product.

## Children

_None yet: status `open`._
