---
id: UNC.INST.IEMPS
title: Instrument Error Model Parameter Sets for virtual-CMM simulation
level: 3
parent: UNC.INST
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-007]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA]
cli: [fid unc import-iemps]
mcp: [unc_import_iemps]
journal: [unc.import_iemps@1]
updated: 2026-11-16
---

# UNC.INST.IEMPS: Instrument Error Model Parameter Sets for virtual-CMM simulation

## Purpose

Ingests Instrument Error Model Parameter Sets: maker- or user-supplied parameter sets (geometric error maps, probing parameters, thermal parameters) that drive an ISO/TS 15530-4 virtual-CMM simulation. An IEMPS is only bindable for simulation once it is anchored to a passing ISO 10360-2 acceptance or reverification record for the same machine.

## Scope

**In:**
- IEMPS data format as an `instrument-model/v1` kind `iemps` extension (error-map grids, probe parameters, thermal terms)
- `unc.import_iemps@1` (W2) with anchoring check
- The anchoring record reference: machine identity, test date, verdict, record digest

**Out:**
- Running the simulation (owned by `UNC.PROP.MCM`)
- Signature verification of maker documents (owned by `TRUST`)
- Evaluating the ISO 10360-2 test itself (owned by `CMM` or `HAL` verification scripts)

## Requirements

- **REQ-UNC.INST.IEMPS-001** When an IEMPS is imported, the system shall verify that it references an ISO 10360-2 acceptance or reverification record for the same machine identity with a passing verdict.
- **REQ-UNC.INST.IEMPS-002** If the referenced record is missing, failed, for a different machine, or older than the IEMPS's declared validity period, then the IEMPS shall be stored but marked `not_bindable` with the reason, and binding it for simulation shall fail.
- **REQ-UNC.INST.IEMPS-003** Every IEMPS shall carry its source (maker or user), version and digest, and results from simulations using it shall reference that digest.
- **REQ-UNC.INST.IEMPS-004** Imported error-map grids shall be bounded in size (default 10^6 nodes [unverified]) and fail closed on overflow.

## Interfaces

- **Offers:** IEMPS model kind; `unc.import_iemps@1`; bindable IEMPS for `UNC.PROP.MCM` virtual-CMM models.
- **Consumes:** model format (UNC.INST.SCHEMA); acceptance-test records (CMM or HAL); signatures (TRUST).

## Decomposition hints

FORMAT (grid and parameter schema), ANCHOR (record verification), IMPORT (command).

## Open questions

- Licensing or interfacing PTB's VCMM for ISO/TS 15530-4 is open [unverified].
- No maker publishes an IEMPS format today as far as we know [unverified]; the format may need to be proposed to makers.

## Children

_None yet: status `open`._
