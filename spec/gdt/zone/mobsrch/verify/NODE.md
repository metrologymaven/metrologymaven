---
id: GDT.ZONE.MOBSRCH.VERIFY
title: Envelope post-check of the transform used
level: 4
parent: GDT.ZONE.MOBSRCH
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.ZONE.MOBSRCH-002]
covers: []
depends_on: [GDT.DRF.CHECK, GDT.DRF.MOB]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.ZONE.MOBSRCH.VERIFY: Envelope post-check of the transform used

## Purpose

A solver can return a transform marginally outside the mobility envelope (tolerances, linearisation). This leaf re-checks every transform used against the envelope with `GDT.DRF.CHECK`'s membership test and converts a failure into the typed `envelope_violation`, so no mobility result reaches a ResultRecord with a transform the datums do not permit.

## Scope

**In:** `verify(transform_used, envelope) -> Ok(within) | Err(envelope_violation{max_excess})`; the largest constraint excess reported in metres or radians.
**Out:** the membership test itself (owned by `GDT.DRF.CHECK`); filling the `datum_mobility` result fields (owned by `GDT.DRF.CHECK`).

## Requirements

- **REQ-GDT.ZONE.MOBSRCH.VERIFY-001** The module shall call `check_within(transform_used, envelope)` from `GDT.DRF.CHECK` for every mobility result.
- **REQ-GDT.ZONE.MOBSRCH.VERIFY-002** If the check fails, then the module shall return `envelope_violation` with the index of the most violated envelope row and its excess, and the caller shall return no value.
- **REQ-GDT.ZONE.MOBSRCH.VERIFY-003** If the check passes, then the module shall return the envelope reference and `within_envelope: true` for the result fields.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/zone/mob/verify.json` (envelope |tx| ≤ 0.05 mm; transforms tx = 0.05 mm and 0.05 mm + 1e-6 mm), when verified with the check's 1e-9 m tolerance, then the first passes and the second fails with `envelope_violation{row 0, excess ≈ 1e-9 m}`.
- **AC-2** Given a single-pose envelope and the identical transform, then pass.
- **AC-3** (determinism) Results are bit-identical on all CI platforms.

## Design

- **Registry command(s):** none (internal kernel API).
- **Data structures:** `Verified { envelope_ref, within_envelope: bool }`.
- **Algorithm:** delegate to `check_within`; on failure evaluate each envelope row at the transform in row order and report the largest excess.
- **Errors:** `envelope_violation{row, excess}`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-gps` (module `zone::mobility`).

## Test fixtures

Hand-written envelope and transform fixtures (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel, pure), module `zone::mobility`, implement `verify(transform_used: &Pose, envelope: &MobilityEnvelope) -> Result<Verified, MobError>` using the DRF check API `check_within`; on failure return `envelope_violation{row, excess}` with the most violated row (lowest index on ties) and its excess; on success return the envelope reference and `within_envelope: true`. Tests per the acceptance criteria. Constraints: CONSTITUTION §8 (no `unwrap`/`expect`/`panic`/`unsafe`), KNP-1 (no FMA, total-order max, NaN is an error).

## Open questions

- The membership tolerance (1e-9 m, 1e-9 rad) is set by `GDT.DRF.CHECK` [unverified there].

## Children

_Leaf: none._
