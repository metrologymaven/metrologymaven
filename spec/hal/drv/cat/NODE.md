---
id: HAL.DRV.CAT
title: Device-support catalog and generated support page
level: 3
parent: HAL.DRV
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.DRV-001, REQ-HAL.DRV-002, REQ-HAL.DRV-003, REQ-HAL.DRV-006]
covers: []
depends_on: [HAL.SIM.CONF, HAL.IDL.PROFILE]
cli: [fid device list-catalog, fid device show-catalog]
mcp: [device_list_catalog, device_show_catalog]
journal: [device.list_catalog@1, device.show_catalog@1]
updated: 2026-11-16
---

# HAL.DRV.CAT: Device-support catalog and generated support page

## Purpose

The machine-readable catalog with one entry per row of `seeds/devices.md` (I++ rows included, supplied by `HAL.IPP.DIAL`, and simulator rows supplied by `HAL.SIM`), the rules that compute each entry's status from conformance reports and recorded vendor terms, and the support page generated for every release. It is the single honest answer to "does it work with my instrument?".

## Scope

**In:**
- Catalog entry schema: DEV ID, device class, integration path (`IPP`, `SDK`, `FILE`, `STD`), tier, capabilities, target phase, tested firmware range, conformance-report digest, reachable trust grade, instrument profile references, vendor-terms record, status
- Status rules: `supported | experimental | planned | blocked`
- Vendor-terms records (who recorded them, when, what they allow), maintained by maintainers
- Generated device-support page per release, never hand-edited
- `device.list_catalog@1` and `device.show_catalog@1` (R0)
- A CI check that every DEV row has exactly one catalog entry

**Out** (and which node owns it instead):
- Drivers themselves (owned by sibling children and `HAL.IPP`)
- Conformance reports (owned by `HAL.SIM.CONF`)
- The public website that hosts the page (owned by `OUTR`)

## Requirements

- **REQ-HAL.DRV.CAT-001** CI shall fail if any row of `seeds/devices.md` lacks a catalog entry or has more than one.
- **REQ-HAL.DRV.CAT-002** An entry's status shall be `supported` only when a valid conformance report for the current driver version covers every capability the entry lists.
- **REQ-HAL.DRV.CAT-003** If an entry's integration path is `SDK` and no maintainer-recorded vendor-terms record exists, then its status shall be `blocked` and the page shall state that vendor terms are unverified.
- **REQ-HAL.DRV.CAT-004** When a release is built, the system shall generate the support page from the catalog and reports, and CI shall fail if the committed page differs from the generated one.
- **REQ-HAL.DRV.CAT-005** `device.list_catalog@1` and `device.show_catalog@1` shall be R0 and return entries sorted by DEV ID with their status and the reason for it.

## Interfaces

- **Offers:** catalog data; generated page; the two R0 commands.
- **Consumes:** `HAL.SIM.CONF` reports; `HAL.IDL.PROFILE` references; entries from sibling driver children and `HAL.IPP.DIAL`.

## Decomposition hints

SCHEMA (entry schema and CI completeness check; likely a leaf), STATUS (status rules; likely a leaf), TERMS (vendor-terms records), PAGE (page generator), CMDS (R0 commands).

## Open questions

- Every DEV row is `[unverified]` in the seed catalog; until maintainers record terms, every `SDK` row is `blocked`.

## Children

_None yet: status `open`._
