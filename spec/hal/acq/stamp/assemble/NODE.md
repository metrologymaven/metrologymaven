---
id: HAL.ACQ.STAMP.ASSEMBLE
title: Stamp assembly, runtime overrides, simulated derivation and ungraded trust
level: 4
parent: HAL.ACQ.STAMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.ACQ.STAMP-002, REQ-HAL.ACQ.STAMP-006]
covers: []
depends_on: [HAL.IDL.STAMP, HAL.ACQ.SESS.REG]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# HAL.ACQ.STAMP.ASSEMBLE: Stamp assembly, runtime overrides, simulated derivation and ungraded trust

## Purpose

The pure function that builds a `stamp/0` from two inputs: what the source declared, and what the runtime knows independently. Runtime facts always win. It derives `simulated` and `source_kind` from the registered source kind, and computes trust either from a grade supplied by the session (P2, from `HAL.PAIR.GRADE`) or, when none is supplied, from an ungraded fallback that can never return `attested`. This is what lets P1a file imports stamp honestly without any pairing machinery.

## Scope

**In:**
- `assemble(declared: DeclaredFields, facts: RuntimeFacts) -> Assembled { stamp, overridden: Vec<FieldName> }`
- Runtime-owned fields: receive time, host digest, session ID, negotiated capability and features, configuration digest, trust, `simulated`, `source_kind`, gaps, payload digest
- `simulated` derivation from the registry's source kind, and propagation from fused inputs
- Trust: external grade if supplied, otherwise the ungraded fallback; the cap for `file` and `entered` sources
- The mapping from registry source kind to stamp `source_kind`

**Out:**
- Stamp schema and validator (owned by `HAL.IDL.STAMP`)
- Storing, validating before emit and journaling (owned by `HAL.ACQ.STAMP.EMIT`)
- Trust-grade rules for live sources (owned by `HAL.PAIR.GRADE`, P2; its output arrives as an input here)
- Calibration flags and claim classification (owned by `HAL.ACQ.POLICY`)

## Requirements

- **REQ-HAL.ACQ.STAMP.ASSEMBLE-001** The function shall set every runtime-owned field from `RuntimeFacts`, ignore any value the source supplies for it, and list each field whose declared value differed in `overridden`, sorted by name.
- **REQ-HAL.ACQ.STAMP.ASSEMBLE-002** When the registered source kind is `simulator`, `emulator` or `playback`, the function shall set `simulated = true` regardless of the declaration; when any fused input batch is simulated, the fused stamp shall also be `simulated = true`.
- **REQ-HAL.ACQ.STAMP.ASSEMBLE-003** The function shall map registry kinds `driver`, `simulator`, `emulator` and `playback` to stamp `source_kind: device` and kind `file` to `source_kind: file`; `source_kind: entered` shall be produced only when the facts state an operator-keyed reading without device identity.
- **REQ-HAL.ACQ.STAMP.ASSEMBLE-004** When `RuntimeFacts` carries no external grade, the function shall set trust to `declared` if the source asserts a device identity and `unknown` otherwise, and shall never set `attested`.
- **REQ-HAL.ACQ.STAMP.ASSEMBLE-005** When `RuntimeFacts` carries an external grade, the function shall use its trust value, except that for `source_kind` `file` or `entered` the trust shall be capped at `declared`.
- **REQ-HAL.ACQ.STAMP.ASSEMBLE-006** Given identical inputs, the function shall return byte-identical canonical stamps on every supported platform.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/acq/stamp/assemble/linescan_declared.json` (source claims `simulated: false`, `trust: attested`, its own receive time) and facts with kind `simulator` and no grade, when assembled, then `simulated` is true, trust is `declared`, the receive time is the runtime's, and `overridden` lists `receive_time`, `simulated` and `trust`.
- **AC-2** Given facts with kind `file`, an external grade `attested` and identity asserted, then trust is `declared` and `source_kind` is `file`.
- **AC-3** Given facts with kind `driver`, no identity and no grade, then trust is `unknown`.
- **AC-4** Given facts with kind `driver` and an external grade `attested`, then trust is `attested` and `simulated` is false.
- **AC-5** Given a fused batch whose pose input was simulated and profile input was not, then the fused stamp is `simulated: true`.
- **AC-6** Given the AC-1 inputs, when the canonical stamp is digested on Windows, Linux and macOS, then the digests are identical and equal `linescan_declared.stamp.sha256`.
- **AC-7** Given the exhaustive table `fixtures/hal/acq/stamp/assemble/trust_table.json` (5 source kinds x grade present or absent x identity asserted or not), then every output trust equals the expected row and none of the no-grade rows is `attested`.

## Design

- **Registry commands:** none; called by `HAL.ACQ.STAMP.EMIT` callers (`HAL.ACQ.INGEST.SINK`, `HAL.ACQ.FILEDEV`, `HAL.IPP`).
- **Data structures:** `DeclaredFields` (driver-supplied subset of `stamp/0`), `RuntimeFacts { session_id, source: SourceRecord, receive_time_ns, host_digest, negotiated, config_digest, external_grade: Option<Trust>, identity_asserted: bool, operator_keyed: bool, gaps, payload_digest, fused_inputs_simulated: bool }`, `Assembled`.
- **Algorithm:** start from declared fields, overwrite runtime-owned fields, derive `simulated` (kind in {simulator, emulator, playback} OR fused input simulated), map source kind, compute trust (grade or fallback, then cap), collect overrides.
- **Errors:** none; schema validation happens in EMIT.
- **Determinism:** D0; no clock reads (receive time is an input), BTreeMap only.
- **Performance budget:** under 20 us per stamp on reference hardware [unverified until measured].
- **Crate:** `fid-hal-runtime` (module `stamp::assemble`), layer L2. It deliberately has no dependency on `fid-hal-pair`: the grade arrives as a plain `Trust` value.

## Test fixtures

Project-authored JSON under `fixtures/hal/acq/stamp/assemble/` (CC-BY-4.0), including the exhaustive trust table.

## Build prompt

> In crate `fid-hal-runtime`, module `stamp::assemble`, implement `assemble(declared: &DeclaredFields, facts: &RuntimeFacts) -> Assembled { stamp: ProvenanceStamp, overridden: Vec<FieldName> }`. Runtime-owned fields (receive time, host digest, session ID, negotiated capability and features, configuration digest, trust, `simulated`, `source_kind`, gaps, payload digest) always come from `facts`; list differing declared values in `overridden` (sorted). `simulated = true` when the registry source kind is simulator, emulator or playback, or any fused input was simulated. Map kinds driver/simulator/emulator/playback -> `device`, file -> `file`, and produce `entered` only when `facts.operator_keyed` is true and no identity is asserted. Trust: `facts.external_grade` if present (capped at `declared` for file and entered), else `declared` if identity asserted and `unknown` otherwise, never `attested`. Do not depend on `fid-hal-pair`. `ProvenanceStamp` comes from `fid-hal-idl` (`stamp/0`); `SourceRecord` from the `registry` module. Pure function, no clock reads, BTreeMap only, no panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-7 with fixtures under `fixtures/hal/acq/stamp/assemble/`; AC-6 on all three OS runners.

## Open questions

- None.

## Children

_Leaf: none._
