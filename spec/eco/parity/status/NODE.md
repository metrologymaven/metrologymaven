---
id: ECO.PARITY.STATUS
title: Per-app status and known-gaps report
level: 3
parent: ECO.PARITY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.PARITY-005]
covers: []
depends_on: [ECO.PARITY.TRACE, ECO.PARITY.GEN, ECO.APPS.MANIFEST, ECO.APPS.LIST]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.PARITY.STATUS: Per-app status and known-gaps report

## Purpose

Generates `status.json` and `status.md`: for each app in the app list, its declared status, its live/total parity over the seed rows in that app's scope, every in-scope row that is not live, and the known gaps declared in its manifest. OUTR renders it on the website and docs portal; this node only produces it (REQ-ECO-010).

## Scope

**In:**
- Deciding which seed rows are in an app's scope (from the app's enabled command namespaces and the trace)
- Joining declared status and known gaps from the manifests' exported status data
- Apps listed but without a manifest yet, shown as `planned`
- Consistency findings: a declared gap that is in fact live, or a non-live row not declared as a gap

**Out** (and which node owns it instead):
- The live rule (owned by `ECO.PARITY.TRACE`)
- Header, rendering conventions and release attachment (shared with `ECO.PARITY.GEN`)
- Manifest content (owned by the per-app nodes under `ECO.APPS`)
- Rendering on the website (owned by `OUTR`)

## Requirements

- **REQ-ECO.PARITY.STATUS-001** The status report shall list each app from the app list with its declared status, or `planned` if it has no manifest.
- **REQ-ECO.PARITY.STATUS-002** For each app, the report shall give live and total counts over the seed rows whose covering leaves name a command in a namespace the app enables.
- **REQ-ECO.PARITY.STATUS-003** The report shall list, per app, every in-scope seed row that is not live, with its reason code.
- **REQ-ECO.PARITY.STATUS-004** If a manifest declares a known gap for a seed row that is live, or a seed row in the app's scope is not live and not declared as a gap, then the report shall list it as a consistency finding.
- **REQ-ECO.PARITY.STATUS-005** The status report shall use the same generated header and determinism rules as the parity report.

## Interfaces

- **Offers:** `status.json`, `status.md` per release.
- **Consumes:** `trace.json` (ECO.PARITY.TRACE); `apps-status.json` and effective namespaces (ECO.APPS.MANIFEST); app list (ECO.APPS.LIST).

## Decomposition hints

SCOPE (seed rows per app), JOIN (status and gaps), CONSIST (consistency findings), RENDER.

## Open questions

- Seed rows are per incumbent product (GOM, PW, PCD, SA). A simpler first rule may map seed prefixes to apps (GOM and PW to Inspect, PCD to Probe, SA to Network); REQ-002's namespace rule is more precise but needs the manifests. The choice is for the leaf.

## Children

_None yet: status `open`._
