---
id: SCAN.SECT.DEF
title: Section plane definitions and series
level: 3
parent: SCAN.SECT
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
built_in: [fid-scan-sect]
built_pr: 672
satisfies: [REQ-SCAN.SECT-001, REQ-SCAN.SECT-002]
covers: [PW.CMP.02]
depends_on: [GEO.FRAME, KRN.CONS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# SCAN.SECT.DEF: Section plane definitions and series

## Purpose

The `SectionDef` type and the pure function that expands it into an ordered list of cutting planes: a single plane, a parallel series with spacing, a radial series about an axis, or a plane taken from a CAD planar face or a constructed plane. Every section command takes a `SectionDef`, so the series rules are written once and stored with each section element.

## Scope

**In:**
- `SectionDef` variants: `Single(plane)`, `Parallel { base plane, spacing, count or extent }`, `Radial { axis point, axis direction, start reference direction, angular step, count or sweep }`, `FromElement(plane element ref)`
- Expansion into an ordered list of planes (point and unit normal in the section's frame)
- Validation and canonical digest of a definition

**Out** (and which node owns it instead):
- Intersecting the planes with data or CAD (owned by `SCAN.SECT.CUT`)
- Constructed planes and CAD plane extraction (owned by `KRN.CONS` and `GEO.CAD`)
- Non-planar section surfaces (not in P1b; see Open questions)

## Requirements

- **REQ-SCAN.SECT.DEF-001** The expansion function shall turn a `Parallel` definition with base plane (p₀, n), spacing s in metres and count k into exactly k planes with points p₀ + i·s·n for i = 0 … k−1 and normal n.
- **REQ-SCAN.SECT.DEF-002** The expansion function shall turn a `Radial` definition with axis (a, d), reference direction r ⟂ d, step θ in radians and count k into exactly k planes containing the axis, the i-th plane's normal being d × (r rotated by i·θ about d), normalised.
- **REQ-SCAN.SECT.DEF-003** A `FromElement` definition shall resolve to the plane of the referenced planar CAD face or constructed plane, and shall record that element's ID and digest so the section recomputes when it changes.
- **REQ-SCAN.SECT.DEF-004** If a normal or axis direction is not unit length within 1e-12, the spacing or step is not positive, the count is zero or above 10 000, or a referenced element is not planar, then validation shall fail with a typed error.
- **REQ-SCAN.SECT.DEF-005** Expansion shall be pure and determinism class D0, with trigonometry only through the `fid-num` facade.

## Acceptance criteria

- **AC-1** Given a parallel definition with base plane z = 0, spacing 0.01 m and count 5, when expanded, then planes are at z = 0, 0.01, 0.02, 0.03, 0.04 m exactly as f64 products i·s, with normal (0,0,1).
- **AC-2** Given a radial definition about the z axis with r = (1,0,0), step π/2 and count 4, then the normals are (0,1,0), (−1,0,0), (0,−1,0), (1,0,0) within 1e-15.
- **AC-3** Given `FromElement` referencing a planar face of the synthetic B-rep box `fixtures/scan/sect/def/brep_box.txt`, the plane equals the face plane and the definition records the face's digest.
- **AC-4** (error path) Given spacing 0, count 20 000 or a cylindrical face, validation fails with `scan.sect.bad_definition`.
- **AC-5** Given the same definition, the canonical digest is identical across runs and platforms.

## Design

- **API** (`fid-scan-sect::def`): `SectionDef` enum, `fn validate(&SectionDef, &ElementResolver) -> Result<(), SectError>`, `fn expand(&SectionDef, &ElementResolver) -> Result<Vec<Plane>, SectError>`, `fn digest(&SectionDef) -> Digest` (RFC 8785 canonical JSON + SHA-256). No registry command of its own; consumed by `compare.section_create@1`.
- **Algorithm:** closed-form expansion as in REQ-001/002; rotation by Rodrigues' formula with `fid-num` `sin`/`cos`.
- **Errors:** `scan.sect.bad_definition` with a reason field.
- **Determinism:** D0.
- **Crate:** `fid-scan-sect` (kernel tier, KNP-1).

## Test fixtures

- Inline definitions in unit tests; `fixtures/scan/sect/def/` (`cases.txt`, the B-rep box `brep_box.txt` and its face planes `box_faces.txt`, all from `make_def_cases.py`) for `FromElement` and the digests. The `fixtures/scan/cpt_box_offset` STEP fixture belongs to SCAN.CMP.CPT and is not used here.

## Build prompt

> In the Rust crate `fid-scan-sect`, module `def`, implement the `SectionDef` enum (`Single`, `Parallel`, `Radial`, `FromElement`) with `validate`, `expand` (ordered `Vec<Plane>` of point + unit normal in metres) and canonical `digest`. Parallel: k planes at p0 + i·s·n. Radial: k planes containing the axis with normal d × rot(r, i·θ, d), normalised. FromElement: resolve a planar CAD face or constructed plane through a resolver trait and record its ID and digest. Validation rejects non-unit directions (1e-12), non-positive spacing or step, count 0 or > 10 000, and non-planar references with `scan.sect.bad_definition`.
>
> Constraints: pure, D0, KNP-1 (no FMA, `fid-num` for trig, no std HashMap/HashSet), no panic/unwrap/unsafe. Tests: the five acceptance criteria.

## Open questions

- Non-planar section surfaces (cylindrical, curve-normal) are deferred beyond P1b, as proposed by SCAN.SECT; adding them is a new variant here plus support in `SCAN.SECT.CUT`.
- Build choices in `fid_scan_sect::def`, for review: a `FromElement` CAD face resolves through GEO.CAD.NOMINAL.DIRECT `map_face` to the placement origin and the out-of-material normal (the axis for sense `Same`, its exact negation for `Reversed`); a NURBS face, even a flat one, is not planar here (recognition is GEO.CAD.NOMINAL.RECOG's). GEO.CAD gives a face no digest of its own, so the recorded digest of a CAD face is the body digest: an edit to any face of the body makes the definition stale (a needless recompute is possible, a missed one is not). A constructed plane records the KRN.CONS output digest. `validate` and `expand` refuse a definition whose recorded digest differs from the element's current one (`element_stale`), so a section is recomputed from a re-recorded definition rather than silently following the edited element.
- Span rules not fixed by this node: a parallel `extent` is the closed range `[0, e]` (offsets `i s <= e + e 1e-12`); a radial `sweep` is the half-open range `[0, w)` (angles `i theta < w - 1e-12`), because a radial series is periodic and a sweep of 2 pi must not repeat its first plane; a radial series whose last angle reaches a full turn (`(k - 1) theta >= 2 pi - 1e-12`) is refused (`radial_full_turn`); the reference direction must be perpendicular to the axis within 1e-12 (`not_perpendicular`). Parallel normals are kept as given (unit within 1e-12, not re-normalised), radial normals are normalised.
- The canonical JSON (`fid.scan.sect.def/1`) is a digest preimage with no decoder: a stored definition is rebuilt through the constructors, and `FromElement::recorded` carries the stored digest as a claim that is checked on every `validate` and `expand`. `-0.0` is stored as `+0.0`, so a definition has one encoding.
- AC-3 and Test fixtures were changed in the build PR (maintainer decision on #672): the box is this leaf's own B-rep builder script `fixtures/scan/sect/def/brep_box.txt`, because `cpt_box_offset` is SCAN.CMP.CPT's STEP fixture (CC-BY-4.0) and stays untouched.
- Frame (REQ-001, REQ-002; needed by SCAN.SECT.CUT REQ-006): every `SectionDef` carries a required `fid-geo-frame` `FrameId`, written as the required `frame` key of the canonical JSON. A `FromElement` records the element's frame as its resolver reports it (`ModelResolver::add_body` and `add_constructed` take the frame, since neither a B-rep body nor a constructed element names one); `SectionDef::from_element` takes it, `SectionDef::new` refuses another (`frame_mismatch`), and expansion refuses an element whose frame changed.
- Format `fid.scan.sect.def/1` fixes SI units: every coordinate, spacing and extent in metres, every step and sweep in radians.
- Coincident planes: an expanded plane bit-identical (point and normal) to the one before it is refused (`coincident_planes`), and a radial step at or below `ANGLE_SLACK` (1e-12 rad) is refused (`step_too_small`).

## Children

_Leaf: none._
