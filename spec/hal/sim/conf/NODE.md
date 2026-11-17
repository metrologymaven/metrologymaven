---
id: HAL.SIM.CONF
title: Conformance suites and signed conformance reports
level: 3
parent: HAL.SIM
kind: module
status: open
phase: P2
size: XL
owners: []
claimed_by:
satisfies: [REQ-HAL.SIM-006, REQ-HAL.SIM-007, REQ-HAL.SIM-008]
covers: []
depends_on: [HAL.IDL.BASE, HAL.SIM.IPPSIM, TRUST.SIGN]
cli: [fid device run-conformance, fid device show-conformance]
mcp: [device_run_conformance, device_show_conformance]
journal: [device.run_conformance@1, device.show_conformance@1]
updated: 2026-11-16
---

# HAL.SIM.CONF: Conformance suites and signed conformance reports

## Purpose

The test suites that decide whether a driver, client or controller conforms to a capability package, and the signed reports that record the result. Client conformance tests any I++ client (ours included) against the simulator; server conformance lets a controller maker or customer test a real controller; the same structure covers every HAL capability. Reports feed trust grading and the device-support catalog.

## Scope

**In:**
- Suite runner and suite format, one suite per conformance profile (`Device-core@1`, `CncMotion-core@1`, `LineScan-core@1`, `Gauge-core@1`, ...)
- I++ client-conformance categories: syntax and framing; session state and reconnection; tools, probes and racks; point measurement and scanning; events, abort and hold; units, frames and head angles; robustness (link drop mid-move, slow consumer, out-of-order events); artefact runs labelled as checks
- I++ server-conformance mode against a real controller
- Measurements of hold latency, look-ahead depth and behaviour on disconnect and client death
- Signed machine-readable reports (subject digests, suite version, per-test outcomes), signed through `TRUST`
- `device.run_conformance@1` (I1) and `device.show_conformance@1` (R0)

**Out** (and which node owns it instead):
- Signature formats and keys (owned by `TRUST`)
- Using reports to grade trust (owned by `HAL.PAIR.GRADE`) or set catalog status (owned by `HAL.DRV.CAT`)
- A public certification programme (P4; owned by `ECO` and `OUTR`)

## Requirements

- **REQ-HAL.SIM.CONF-001** Every conformance profile defined in `HAL.IDL` shall have an executable suite in this node with one test per profile item.
- **REQ-HAL.SIM.CONF-002** When `device.run_conformance@1` completes, the system shall emit a report recording the subject digests (driver or client, firmware where known), the suite version and the outcome of every test, signed through `TRUST`.
- **REQ-HAL.SIM.CONF-003** The I++ client suite shall measure and report hold latency, look-ahead depth and behaviour on TCP disconnect and on client death.
- **REQ-HAL.SIM.CONF-004** Artefact runs in any suite shall be labelled as checks and never as acceptance or verification tests under ISO 10360.
- **REQ-HAL.SIM.CONF-005** The suites shall contain no text from any licensed specification and shall be releasable under a permissive licence independent of the product licence [unverified pending D-001 follow-up].
- **REQ-HAL.SIM.CONF-006** `device.run_conformance@1` shall be class I1 when the subject is a simulator, and running server conformance against a real controller shall require the motion path of `CMM` execution (never MCP).

## Interfaces

- **Offers:** suites; runner; signed reports; the two commands.
- **Consumes:** `HAL.IDL` conformance profiles; `HAL.SIM.IPPSIM` and other simulators; `TRUST.SIGN`.

## Decomposition hints

RUNNER (runner and suite format), IPPCLI (I++ client suite), IPPSRV (I++ server suite), CAPS (capability suites, one per profile), REPORT (signed reports and show command; likely a leaf).

## Open questions

- Concept doc 08 proposes an in-toto Statement with a test-result predicate in a DSSE envelope; the envelope choice belongs to `TRUST` [unverified].
- Server conformance on a real controller moves the machine; its side-effect class is M1 through the Approval Station, so it is not an I1 command; this needs a dedicated command when `CMM.EXEC` is split.

## Children

_None yet: status `open`._
