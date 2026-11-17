---
id: HAL.IPP.DIAL
title: Controller dialect profiles, conformance gate and controller matrix
level: 3
parent: HAL.IPP
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.IPP-002, REQ-HAL.IPP-007]
covers: [DEV.CMM.01, DEV.CMM.02]
depends_on: [HAL.IPP.SESS, HAL.SIM.CONF, HAL.SIM.IPPSIM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# HAL.IPP.DIAL: Controller dialect profiles, conformance gate and controller matrix

## Purpose

Makes the I++ client honest about which controllers it works with. It holds a dialect profile per controller family (known server behaviours the client must tolerate), runs the client-conformance suite in CI on every commit, and publishes a controller and option matrix whose rows feed the device-support catalog with path `IPP`, tested firmware range and conformance-report digest.

## Scope

**In:**
- Client-side dialect profiles (tolerated deviations, timing, unsupported commands), sourced from public behaviour or own observation
- CI gate: every client-conformance category against the simulator at L0 and L1 on every commit
- Controller and option matrix: family, firmware range tested, options (scanning, rotary, heads, line laser), conformance report digest
- Catalog rows for DEV.CMM.01 and DEV.CMM.02 delivered to `HAL.DRV.CAT`

**Out** (and which node owns it instead):
- Simulator-side dialects (owned by `HAL.SIM.IPPSIM`)
- The suites (owned by `HAL.SIM.CONF`)
- The generated device-support page (owned by `HAL.DRV.CAT`)

## Requirements

- **REQ-HAL.IPP.DIAL-001** CI shall run every client-conformance category against the I++ simulator at L0 and L1 on every commit and fail the build on any failed test.
- **REQ-HAL.IPP.DIAL-002** Each supported controller family shall have a dialect profile that names its source (public behaviour or own observation) and contains no behaviour learned under NDA.
- **REQ-HAL.IPP.DIAL-003** Each supported controller family shall have a catalog row with path `IPP`, tested firmware range and the digest of its latest conformance report.
- **REQ-HAL.IPP.DIAL-004** If a controller family has no valid conformance report for the current client version, then its catalog status shall not be `supported`.

## Interfaces

- **Offers:** dialect profiles to `HAL.IPP.SESS`; matrix and catalog rows to `HAL.DRV.CAT`.
- **Consumes:** `HAL.SIM.CONF` suites and reports; `HAL.SIM.IPPSIM`.

## Decomposition hints

GATE (CI conformance gate; likely a leaf), PROF (dialect profile format), MATRIX (controller matrix and catalog rows).

## Open questions

- No partner controller is named yet; the first family depends on partner access in P2.

## Children

_None yet: status `open`._
