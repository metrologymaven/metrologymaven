---
id: GDT.DRF.CHECK.FIELDS
title: Datum-mobility result fields, RMB membership and frame flag
level: 4
parent: GDT.DRF.CHECK
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-GDT.DRF.CHECK-001, REQ-GDT.DRF.CHECK-002, REQ-GDT.DRF.CHECK-003]
covers: []
depends_on: [GDT.DRF.BUILD, GDT.CHAR.RECORD, GEO.FRAME]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# GDT.DRF.CHECK.FIELDS: Datum-mobility result fields, RMB membership and frame flag

## Purpose

Every result evaluated in a DRF must say which transform was used and whether it lies inside the mobility envelope; every result evaluated in an alignment instead of its DRF must say so. This leaf owns the `datum_mobility` block and the `frame` flag that evaluating children add to the ResultRecord, and the P1a membership case: with all datums at RMB the envelope is the single DRF pose, so membership is pose equality within tolerance. General envelopes are delegated to `GDT.DRF.CHECK.MEMB` through a trait, so this P1a leaf has no dependency on the P1b envelope type.

## Scope

**In:** `DatumMobilityFields { drf_ref, transform_used, envelope_ref, within_envelope, group_id? }`; `FrameFlag { Drf, AlignmentSubstitute{alignment_ref} }`; `rmb_within(transform_used, drf_pose)` with 1e-9 m and 1e-9 rad tolerances [unverified]; a `MembershipTest` trait that `GDT.DRF.CHECK.MEMB` implements for non-trivial envelopes; population of these fields through the ResultRecord builder.
**Out:** general membership (owned by `GDT.DRF.CHECK.MEMB`); envelope construction (owned by `GDT.DRF.MOB`); the ResultRecord builder (owned by `GDT.CHAR.RECORD`).

## Requirements

- **REQ-GDT.DRF.CHECK.FIELDS-001** Every ResultRecord evaluated in a DRF shall carry `datum_mobility` with the DRF reference, the transform used, the envelope reference and the within-envelope boolean.
- **REQ-GDT.DRF.CHECK.FIELDS-002** While no datum carries MMB or LMB, the envelope reference shall be the DRF pose itself, and within-envelope shall be true exactly when the transform used differs from the DRF pose by at most 1e-9 m in translation and 1e-9 rad in rotation angle [unverified].
- **REQ-GDT.DRF.CHECK.FIELDS-003** If a characteristic is evaluated in an alignment rather than its DRF, then the ResultRecord shall carry `frame: alignment-substitute` with the alignment reference and no `datum_mobility` block.
- **REQ-GDT.DRF.CHECK.FIELDS-004** If an envelope other than a single pose is supplied and no `MembershipTest` implementation is registered, then the function shall return `membership_unavailable` and the result shall not claim within-envelope.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/gdt/drf/check_rmb.json` (DRF pose and transform used equal), when the fields are populated, then within-envelope is true and the envelope reference equals the DRF pose reference.
- **AC-2** Given a transform used translated by 2e-9 m from the DRF pose, then within-envelope is false; translated by 0.5e-9 m, then true.
- **AC-3** Given a characteristic evaluated in alignment `ALN-1`, then the record has `frame: alignment-substitute`, `alignment_ref: ALN-1` and no `datum_mobility` key.
- **AC-4** (error path) Given a non-trivial envelope and no registered `MembershipTest`, then `membership_unavailable`.
- **AC-5** (determinism) Output JSON is byte-identical across CI platforms.

## Design

- **Registry command(s):** none (helpers called by evaluating children).
- **Data structures:** as in Scope; `trait MembershipTest { fn within(&self, transform, envelope_ref) -> Result<bool, MembershipError>; }`.
- **Algorithm:** relative transform = DRF pose⁻¹ · transform used; translation norm and rotation angle (via `fid-num` atan2 of the axis-angle form) compared with the tolerances.
- **Errors:** `membership_unavailable`, `nan_input`.
- **Determinism:** D0.
- **Performance budget:** < 1 µs per record [unverified].
- **Crate:** `fid-gps` (module `drf::check`).

## Test fixtures

Analytic poses (CC0, in-repo).

## Build prompt

> In crate `fid-gps` (kernel), module `drf::check`, implement `DatumMobilityFields`, `FrameFlag { Drf, AlignmentSubstitute{alignment_ref} }` and their population through the ResultRecord builder. For RMB-only DRFs the envelope is the DRF pose: `within_envelope` is true exactly when the relative transform (DRF pose inverse times transform used) has translation norm <= 1e-9 m and rotation angle <= 1e-9 rad (angle through `fid-num`). Define `trait MembershipTest` for general envelopes (implemented elsewhere); with a non-trivial envelope and no implementation, return `membership_unavailable`. A result evaluated in an alignment carries `frame: alignment-substitute` and no `datum_mobility` block. Tests per acceptance criteria. Constraints: CONSTITUTION §8, KNP-1 (no FMA, `fid-num` transcendentals, NaN is an error), no std `HashMap`/`HashSet`.

## Open questions

- The tolerances 1e-9 m and 1e-9 rad are inherited [unverified]; confirm against ORS-DRF case precision.

## Children

_Leaf: none._
