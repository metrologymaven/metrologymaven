---
id: HAL.ACQ.STAMP
title: Provenance stamping, raw-batch storage and batch journaling
level: 3
parent: HAL.ACQ
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ-001, REQ-HAL.ACQ-002]
covers: []
depends_on: [HAL.IDL.STAMP, HAL.PAIR.GRADE, CORE.JRN, CORE.STO]
cli: []
mcp: []
journal: [device.acquisition_batch]
updated: 2026-11-16
---

# HAL.ACQ.STAMP: Provenance stamping, raw-batch storage and batch journaling

## Purpose

The single writer of `AcquisitionBatch` events. It assembles the provenance stamp from what the driver declared and what the runtime knows independently, stores the raw batch by digest, journals the batch, and makes the `simulated` flag indelible. Holding this invariant in one module is what lets every downstream result trust its observations' provenance.

## Scope

**In:**
- Stamp assembly: driver-declared fields plus runtime-known fields (host digest, receive time, session, negotiation, configuration digest, trust grade, `simulated`, `source_kind`)
- Raw-batch storage in the content-addressed store and payload digest
- Journaling `device.acquisition_batch` events through the command bus
- `simulated` derivation from the serving source kind (simulator, emulator, playback) and its indelibility on the batch
- The stamp-before-journal check: no batch event without a complete, validated stamp

**Out** (and which node owns it instead):
- The stamp schema (owned by `HAL.IDL.STAMP`)
- The trust-grade rules (owned by `HAL.PAIR.GRADE`)
- Propagating `simulated` to derived results and blocking their release (owned by `TRUST` and `RPT`)
- Calibration flags and `ExternalClaim` classification (owned by `HAL.ACQ.POLICY`)

## Requirements

- **REQ-HAL.ACQ.STAMP-001** The stamping module shall be the only code path that can emit a `device.acquisition_batch` event, and it shall validate the stamp against `stamp/0` before emitting.
- **REQ-HAL.ACQ.STAMP-002** When the serving source is registered as a simulator, emulator or playback, the module shall set `simulated = true` regardless of what the source declares.
- **REQ-HAL.ACQ.STAMP-003** No registry command shall be able to modify a journaled batch's stamp; a correction shall be a new event that references the batch and can never set `simulated` from true to false.
- **REQ-HAL.ACQ.STAMP-004** The module shall store the raw payload bytes in the content-addressed store before journaling, and the stamp's payload digest shall equal the stored object's digest.
- **REQ-HAL.ACQ.STAMP-005** If stamp validation fails, then the module shall refuse to journal the batch, record a gap with reason `REJECTED`, and raise a `device.event` naming the failing field.
- **REQ-HAL.ACQ.STAMP-006** The runtime-known fields of the stamp (receive time, host digest, session ID, trust grade, `simulated`) shall be computed by the runtime and shall override any value a driver supplies for them.

## Interfaces

- **Offers:** `stamp_and_journal(batch, session) -> BatchRef`; `device.acquisition_batch` events; observation handles for `HAL.LIVE`, `LVM`, `SCAN` and `CMM`.
- **Consumes:** `HAL.IDL.STAMP` validator; `HAL.PAIR.GRADE` trust function; `HAL.ACQ.POLICY` flags; `CORE.STO`; `CORE.JRN`.

## Decomposition hints

ASSEMBLE (stamp assembly and override rules), CAS (raw storage), JOURNAL (event emission and stamp-before-journal check), SIMFLAG (simulated derivation and correction events).

## Open questions

- Whether the `simulated` flag must also be enforced at the result level by CORE (a type that cannot be cleared) or only by TRUST release checks is a cross-domain question for `CORE` and `TRUST`.

- This node starts in P1a but its trust-grade input (`HAL.PAIR.GRADE`) is P2. In P1a only file and manual sources exist, which grade `declared` or `unknown` without pairing; the grade function must be available as a library before then, or STAMP uses a P1a fallback that never returns `attested`.

## Children

_None yet: status `open`._
