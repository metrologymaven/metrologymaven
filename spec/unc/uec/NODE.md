---
id: UNC.UEC
title: Uncertainty evidence classes and result uncertainty statement
level: 2
parent: UNC
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC-001, REQ-UNC-002]
covers: []
depends_on: [KRN]
cli: [fid unc show-*, fid unc declare-*, fid unc explain-*]
mcp: [unc_show_*, unc_declare_*, unc_explain_*]
journal: [unc.declare_*]
updated: 2026-11-16
---

# UNC.UEC: Uncertainty evidence classes and result uncertainty statement

## Purpose

Owns the **uncertainty part of every ResultRecord**: the evidence class UEC-0..3, the uncertainty statement (value, coverage factor or interval, method, model ID and digest, input sources), the rules for assigning a class, and the way a missing or declared uncertainty is shown. It exists separately so that every other UNC child, and GDT, writes uncertainty through one typed contract instead of inventing fields. It never computes an uncertainty itself.

## Scope

**In:**
- The UEC-0..3 enumeration and the criteria for assigning each class
- The uncertainty statement schema on ResultRecord (expanded U, k or coverage interval and probability, standard uncertainty, method tag, model ID and digest, contributor sources, `lpu_validated_by_mc` flag)
- UEC-0 handling: "not evaluated" marker, typed reason, suggested fix
- UEC-1 declared uncertainty entered by a person (author, source, date)
- Mapping of the statement into QIF 3.0 results through sanctioned extension hooks
- Read-only explanation queries: contributors, sensitivities, largest contributors (data supplied by `UNC.PROP`)

**Out** (and which node owns it instead):
- Building measurement models and contributor libraries (owned by `UNC.MODEL`)
- Computing LPU and Monte Carlo results (owned by `UNC.PROP`)
- Instrument-derived UEC-1 models (owned by `UNC.INST`)
- Using U in conformance decisions (owned by `UNC.DEC`)
- Clickable display in the viewer (owned by `VIZ`); report layout (owned by `RPT`)

## Requirements

- **REQ-UNC.UEC-001** Every ResultRecord shall carry exactly one evidence class from the set UEC-0, UEC-1, UEC-2, UEC-3.
- **REQ-UNC.UEC-002** If no uncertainty has been evaluated or declared for a result, then the system shall assign UEC-0 and leave every numeric uncertainty field empty rather than filling in a default, zero or estimated value.
- **REQ-UNC.UEC-003** When a UEC-0 result is shown or exported, the system shall label it "not evaluated" and attach a typed reason (`u_missing`, `model_missing`, `instrument_missing`) and at least one suggested fix.
- **REQ-UNC.UEC-004** The uncertainty statement of every UEC-1..3 result shall state the method (`LPU | MCM | ISO15530-3 | ISO15530-4 | DECLARED | SPEC_DERIVED`), the coverage factor or the coverage interval with its coverage probability, the model ID with its content digest, and the unit.
- **REQ-UNC.UEC-005** The system shall assign UEC-2 only when every contributor of the model references a characterized source record (calibration certificate, qualification record or type-A dataset), and shall otherwise assign at most UEC-1.
- **REQ-UNC.UEC-006** The system shall assign UEC-3 only to results produced by an ISO 15530-3 substitution evaluation or an ISO/TS 15530-4 simulation that is recorded as validated against calibrated workpieces.
- **REQ-UNC.UEC-007** When a person declares an uncertainty for a characteristic, the system shall record it as UEC-1 with author, source text and timestamp through a journaled W2 command.
- **REQ-UNC.UEC-008** If an agent declares an uncertainty, then the system shall mark it as an agent proposal that requires human adoption before any result using it is released (CONSTITUTION C5).
- **REQ-UNC.UEC-009** The uncertainty statement shall round-trip through QIF 3.0 export and import without loss of class, method, coverage, model digest or sources.

## Interfaces

- **Offers:** `UncertaintyStatement` and `EvidenceClass` types used by GDT, RPT, TRUST and every UNC child; `unc.show_statement@1` (R0), `unc.explain_result@1` (R0), `unc.declare_uncertainty@1` (W2, journal `unc.declare_uncertainty`).
- **Consumes:** ResultRecord from CORE and GDT; propagation outputs from `UNC.PROP`; instrument model references from `UNC.INST`.

## Decomposition hints

STMT (statement schema and QIF mapping), CLASS (class assignment rules), DECL (declared uncertainty command), EXPL (explain queries and UEC-0 reasons).

## Open questions

- Which QIF 3.0 extension hook carries the method tag and model digest is not yet confirmed [unverified].

## Children

_None yet: status `open`._
