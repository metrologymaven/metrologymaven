---
id: ECO.PARITY
title: Generated parity, coverage and status reports
level: 2
parent: ECO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-006, REQ-ECO-010]
covers: []
depends_on: [CORE, ECO.APPS, ECO.COMM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.PARITY: Generated parity and status reports

## Purpose

Generates, never hand-writes, the reports that say how far the project has come: seed coverage from the spec tree (available now), parity from the live registry manifest against the seed inventories (once code exists), and per-app status and known-gaps data. OUTR renders these on the website and docs portal; this node produces them.

## Scope

**In:**
- The parity generator: registry manifest joined with seed rows, giving live/total per seed file and per area
- The link from a registry command back to the seed IDs it implements (through spec nodes' `covers:`)
- Per-app status and known-gaps report, combining ECO.APPS manifests, spec-node status and parity
- Machine-readable (JSON) and Markdown outputs, marked "generated, do not edit"
- The check that a published report matches a fresh regeneration
- Attaching reports to each release

**Out** (and which node owns it instead):
- Seed-coverage logic inside `scope.py` (owned by `ECO.COMM`; this node consumes its output)
- Rendering on the website and docs portal (owned by `OUTR`)
- Seed inventory content (owned by the `seeds/` maintainers and each domain)
- Release assembly (owned by `ECO.REL`)

## Requirements

- **REQ-ECO.PARITY-001** The parity generator shall report, for each seed file and each seed area, the number of seed rows implemented by at least one registry command against the total in scope.
- **REQ-ECO.PARITY-002** A seed row shall count as live only when a released registry command traces to a spec node that covers it, and that node is a leaf whose acceptance criteria pass in CI.
- **REQ-ECO.PARITY-003** Every generated report shall carry a "generated, do not edit" header plus the registry manifest digest and spec-tree revision it was built from.
- **REQ-ECO.PARITY-004** If a committed or published report differs from a fresh regeneration from the same inputs, then the parity check shall fail.
- **REQ-ECO.PARITY-005** The status report shall list, for each app, its declared status, its live/total parity and every in-scope seed row that is not live.
- **REQ-ECO.PARITY-006** When a release is built, the parity and status reports for that release shall be generated and attached to it.
- **REQ-ECO.PARITY-007** Given the same inputs, the generator shall produce byte-identical output.

## Interfaces

- **Offers:** `parity.json`, `parity.md`, `status.json`, `status.md` per release; the regeneration check.
- **Consumes:** CORE registry manifest; ECO.APPS status and gaps data; `scope.py` coverage output and node status (ECO.COMM); seeds.

## Decomposition hints

GEN (parity generator), TRACE (command-to-seed traceability), STATUS (per-app status and gaps), DRIFT (regeneration check).

## Split rationale

The split follows the **report pipeline**: TRACE decides what is live (the one place the live rule lives), GEN counts and renders it, STATUS regroups it per app, DRIFT proves no published report was edited. TRACE links commands to leaves through each leaf's own `journal:` field, so parity needs nothing extra from the registry beyond command IDs. TRACE, GEN and DRIFT are self-contained repository tools and are leaves; STATUS waits on the app manifest schema.

Alternatives considered:
- **Fold the live rule into the generator** (hint GEN + TRACE as one). Rejected: STATUS needs the same rule, and a second copy would drift.
- **Seed coverage from `scope.py` as a parity input.** Kept separate: `scope.py` coverage (ECO.COMM.SCOPE) is about spec nodes; parity is about released, tested commands. TRACE reads the tree directly.

## Open questions

- Out-of-scope seed rows are shown in a separate column (ECO.PARITY.GEN REQ-001), so readers see what was deliberately dropped.
- Cross-domain: ECO.PARITY.TRACE assumes the CORE.REG registry manifest exposes at least `commands[].id`, and ECO.CI.WF must publish per-leaf acceptance-criterion outcomes; confirm when CORE.REG is expanded.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.PARITY.TRACE | Seed-to-command traceability and the live rule (leaf) | 002 | none |
| ECO.PARITY.GEN | Parity report generator (leaf) | 001 003 006 007 | none |
| ECO.PARITY.DRIFT | Report regeneration (drift) check (leaf) | 004 | none |
| ECO.PARITY.STATUS | Per-app status and known-gaps report | 005 | none |
