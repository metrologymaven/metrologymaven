---
id: MIG.EXPLAIN.VARIANT
title: Single-factor re-evaluation variants per factor
level: 3
parent: MIG.EXPLAIN
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG.EXPLAIN-002]
covers: []
depends_on: [MIG.EXPLAIN.CAT, GDT.FORM.FCORE, GDT.RES.RESOLVE, ALN.XFM.MATRIX, ALN.QA.CMP, KRN.OUT, KRN.NUM]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# MIG.EXPLAIN.VARIANT: Single-factor re-evaluation variants per factor

## Purpose

Knows, for each factor in the catalogue, how to turn a `ProbeCandidate` into a concrete **variant**: the recorded evaluation span of one characteristic with exactly one input or parameter rebound (or, for rounding, a post-processing rule on the kernel value). This is where domain knowledge lives: swapping an association policy for a fit, applying a point filter policy, substituting the incumbent's known transform for our alignment, choosing an alternative datum-interpretation row, restricting the point set to the incumbent's selection, or rounding to the incumbent's displayed decimals. The output is a typed request that `MIG.EXPLAIN.PROBE` executes; this node runs nothing itself.

## Scope

**In:**
- A `VariantBuilder` per factor, producing a `ProbeVariant { span, bindings, parameter_overrides, post_rule? }` for the replay engine
- `FIT_METHOD`: rebind the association policy of the feature fit (e.g. least squares, minimum zone, maximum inscribed, minimum circumscribed) through the `GDT.FORM.FCORE` policy IDs
- `FILTER`: rebind the point-filter or outlier policy through `KRN.OUT` policy IDs and parameters
- `ALIGNMENT`: substitute the imported incumbent alignment (a known transform from `MIG.PROJ.ALIGN`) and record the `ALN.QA.CMP` difference between the two alignments
- `DATUM_INTERPRETATION`: rebind the resolver row chosen by `GDT.RES.RESOLVE` to a named alternative row
- `POINT_SELECTION`: rebind the point set to an explicit index subset
- `ROUNDING`: no re-evaluation; a post rule that rounds the kernel value with `KRN.NUM` decimal rounding to the incumbent's stated decimals
- Applicability check: report `NOT_APPLICABLE` when a factor cannot vary for a characteristic (e.g. datum interpretation on an undatumed form tolerance)

**Out** (and which node owns it instead):
- The candidate model (owned by `MIG.EXPLAIN.CAT`)
- Executing variants and the budget (owned by `MIG.EXPLAIN.PROBE`)
- Fit, resolver and alignment algorithms themselves (owned by `GDT`, `ALN`, `KRN`)

## Requirements

- **REQ-MIG.EXPLAIN.VARIANT-001** For every factor in the catalogue, the system shall build a variant that differs from the recorded evaluation span of the characteristic in exactly the inputs or parameters of that factor, and shall list those differences in the variant.
- **REQ-MIG.EXPLAIN.VARIANT-002** If a factor cannot be varied for a characteristic, then the builder shall return `NOT_APPLICABLE` with a reason code and no variant.
- **REQ-MIG.EXPLAIN.VARIANT-003** An `ALIGNMENT` variant shall use only an alignment imported as a known transform from the incumbent export and shall record the `ALN.QA.CMP` comparison of that transform with ours.
- **REQ-MIG.EXPLAIN.VARIANT-004** A `ROUNDING` variant shall not re-evaluate and shall apply a single rounding to the recorded kernel value using the `KRN.NUM` rounding rule named in the candidate.
- **REQ-MIG.EXPLAIN.VARIANT-005** When two variants are combined for a two-factor probe, the builder shall refuse the combination with `variant_conflict` if both factors rebind the same input.
- **REQ-MIG.EXPLAIN.VARIANT-006** Variant construction shall be a pure function of the recorded span, the candidate and the registry manifest version, with a canonical digest.

## Interfaces

- **Offers:** `build_variant(span, candidate) -> Result<ProbeVariant, NotApplicable>`, `combine(a, b) -> Result<ProbeVariant, VariantConflict>`; `ProbeVariant` type consumed by `MIG.EXPLAIN.PROBE`.
- **Consumes:** `MIG.EXPLAIN.CAT` candidates; `GDT.FORM.FCORE` association policy IDs; `GDT.RES.RESOLVE` alternative rows; `ALN.XFM.MATRIX` known transforms and `ALN.QA.CMP`; `KRN.OUT` filter policies; `KRN.NUM` rounding.
- **Uses data from:** `MIG.PROJ.ALIGN` imported alignments (entities in the document; no code dependency).

## Decomposition hints

FIT (fit-method and filter variants), FRAME (alignment and datum-interpretation variants), SEL (point-selection and rounding variants), COMB (combination and conflict check).

## Open questions

- Several incumbents' default association and filter settings are not documented publicly [unverified]; candidates for them can only be user-selected.
- Whether "point selection" should also cover incumbent-side outlier removal that the export does not record is open; today only explicit index subsets are supported.

## Children

_None yet: status `open`._
