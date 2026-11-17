---
id: HAL.IDL.SENS.STREAM
title: Shared streaming template, vendor-extension rule and batch-size profile items
level: 4
parent: HAL.IDL.SENS
kind: task
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-HAL.IDL.SENS-001, REQ-HAL.IDL.SENS-006, REQ-HAL.IDL.SENS-007]
covers: []
depends_on: [HAL.IDL.BASE, HAL.IDL.UNITS, HAL.IDL.GEN.FMT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.IDL.SENS.STREAM: Shared streaming template, vendor-extension rule and batch-size profile items

## Purpose

The pattern every sensor package follows (configure, start, stream, stop), written once so the eight sensor packages differ only in their payloads. It also holds the two rules common to all of them: vendor extensions must be declared under the vendor's namespace and are always digested, and streaming batches should be 1 to 8 MB or 50 ms of data.

## Scope

**In:**
- A `StreamingPackage` builder: adds `start_stream` and `stop_stream` (D1), `stream_status` (R0) and the standard stream events to a package description
- The common conformance profile items `Stream-common@1`: start/stop idempotence, sequence contiguity, batch size within the recommendation, extension declaration and digesting
- The vendor-extension rule as a package-level validator

**Out:**
- Configuration typing and digest (owned by `HAL.IDL.BASE.CFG`)
- Each package's payloads (owned by `HAL.IDL.SENS.GAUENV`, `HAL.IDL.SENS.LINE`, `HAL.IDL.SENS.PORT`, `HAL.IDL.SENS.AREA`, `HAL.IDL.SENS.SURF`, `HAL.IDL.SENS.CAM`)
- Enforcing admission limits at runtime (owned by `HAL.ACQ.INGEST.SEQ`)

## Requirements

- **REQ-HAL.IDL.SENS.STREAM-001** The builder shall add to a package the operations `start_stream` and `stop_stream` (D1) and `stream_status` (R0), and every sensor package shall be built with it, so all eight packages have describe, configure and acquisition operations.
- **REQ-HAL.IDL.SENS.STREAM-002** If a package description declares a vendor-extension setting whose name is not prefixed by a vendor namespace listed in the package's describe, then the package validator shall fail with `extension_undeclared`.
- **REQ-HAL.IDL.SENS.STREAM-003** The `Stream-common@1` profile shall include an item that passes only if at least 95 % of a 60 s test stream's batches are between 1 MB and 8 MB or span at most 50 ms of device time [unverified].
- **REQ-HAL.IDL.SENS.STREAM-004** The `Stream-common@1` profile shall include items for start/stop idempotence, sequence contiguity, and the presence of every extension value in the configuration digest.

## Acceptance criteria

- **AC-1** Given a demo payload package built with the builder, when exported, then it contains `start_stream`, `stop_stream` (D1) and `stream_status` (R0) and validates as `capability-package/v1`.
- **AC-2** (error path) Given a demo package with extension `secret_mode` (no namespace), then validation fails with `extension_undeclared`.
- **AC-3** Given the batch-size item and a recorded size list `fixtures/hal/idl/sens/stream/sizes_ok.json` (97 % in range), then it passes; `sizes_bad.json` (80 %) fails.
- **AC-4** Given `Stream-common@1`, then it lists the four items of REQ-HAL.IDL.SENS.STREAM-003 and -004 with stable IDs.

## Design

- **Registry commands:** none; library used by every sensor package.
- **Data structures:** `StreamingPackage` builder over `PackageDescription`; `StreamEvent { Started, Stopped { reason }, Gap }`; profile item evaluators over recorded batch summaries (pure functions the suites call).
- **Errors:** `extension_undeclared`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-hal-idl` (module `caps::stream`), layer L0.

## Test fixtures

Project-authored size lists and demo packages under `fixtures/hal/idl/sens/stream/` (CC-BY-4.0).

## Build prompt

> In crate `fid-hal-idl` (L0), module `caps::stream`, implement a `StreamingPackage` builder that adds `start_stream`/`stop_stream` (D1) and `stream_status` (R0) plus standard stream events to a `PackageDescription`; a validator rule failing `extension_undeclared` for extension settings without a declared vendor namespace; and the `Stream-common@1` profile with pure evaluators: batch size (pass if >= 95 % of batches are 1-8 MB or span <= 50 ms), start/stop idempotence, sequence contiguity, extension values present in the configuration digest. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-4.

## Open questions

- The 95 % threshold is a proposal [unverified].

## Children

_Leaf: none._
