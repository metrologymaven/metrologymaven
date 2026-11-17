---
id: OUTR.COMM.METRIC
title: Adoption metric definitions and collection
level: 3
parent: OUTR.COMM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-004, REQ-OUTR.COMM-005]
covers: []
depends_on: [OUTR.COMM.CHAN, ECO.REL]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.COMM.METRIC: Adoption metric definitions and collection

## Purpose

The written definitions of every adoption metric and the collector that gathers them from public signals only: release download counts, repository stars and forks, public channel activity and contributor counts. It is separate from the report so that the definitions and the no-telemetry boundary are reviewed once and reused every quarter.

## Scope

**In:**
- Metric definition file: name, source, collection method, unit, known biases
- Collector that reads public APIs or public pages of the release host, repository host and channels
- Raw snapshot storage with collection date and source digest
- A guard that refuses any source not listed as public in the definitions

**Out** (and which node owns it instead):
- Survey data (owned by `OUTR.COMM.SURVEY`)
- The quarterly report (owned by `OUTR.COMM.REPORT`)
- Product telemetry of any kind (none by default; any opt-in telemetry belongs to `CORE` or `PLAT`, and is never an input here)

## Requirements

- **REQ-OUTR.COMM.METRIC-001** Each adoption metric shall have a definition entry stating its source, collection method, unit and known biases (for example mirror downloads not counted), and the collector shall refuse to compute a metric with no definition.
- **REQ-OUTR.COMM.METRIC-002** The collector shall read only sources marked public in the definition file, and if a configured source is not marked public, then it shall stop and name the source.
- **REQ-OUTR.COMM.METRIC-003** The collector shall never read data collected by the product, including any opt-in telemetry, crash reports or support bundles.
- **REQ-OUTR.COMM.METRIC-004** Each collection run shall store a raw snapshot per metric with the collection timestamp, source URL and a SHA-256 digest of the raw response, so that a quarter's numbers can be recomputed from stored snapshots.
- **REQ-OUTR.COMM.METRIC-005** Contributor counts shall be computed from public repository history and shall count distinct contributors, without storing email addresses.

## Interfaces

- **Offers:** the metric definition file; raw snapshots; per-metric quarterly values for OUTR.COMM.REPORT.
- **Consumes:** ECO.REL release hosts (download counts); the repository host's public statistics; channel activity from the OUTR.COMM.CHAN inventory.

## Decomposition hints

DEF (definition file), COLLECT (collector per source), SNAP (snapshot storage).

## Open questions

- The repository and release hosts are not chosen yet; the collector's sources follow ECO.REL and ECO.REPO.

## Children

_None yet: status `open`._
