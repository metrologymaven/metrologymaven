---
id: GDT.CHAR
title: Characteristic model, dialects and result records
level: 2
parent: GDT
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-GDT-001, REQ-GDT-007]
covers: [GOM.GDT.17, PCD.DIM.02, PCD.DIM.05, PW.GDT.08]
depends_on: [CORE, IO, UNC.UEC, UNC.DEC]
cli: [fid gdt *-characteristic, fid gdt *-defaults-profile, fid gdt *-result]
mcp: [gdt_*_characteristic, gdt_*_defaults_profile, gdt_*_result]
journal: [gdt.*_characteristic, gdt.*_defaults_profile]
updated: 2026-11-16
---

# GDT.CHAR: Characteristic model, dialects and result records

## Purpose

Owns the data that every other GDT child reads and writes: the characteristic (a QIF 3.0 characteristic definition, nominal and item, plus the feature control frame and its modifiers), the per-characteristic **governing spec** (dialect, standard editions and defaults profile), the defaults-profile objects themselves, and the assembly of the **ResultRecord** that every evaluation emits. It exists separately so that dialect discipline and the ResultRecord contract are enforced in one place rather than re-implemented by each tolerance family.

## Scope

**In:**
- Characteristic model on QIF 3.0 (ISO 23952) characteristic definition / nominal / item / measurement, with Fiducial extensions only through QIF `Attributes` hooks (CONSTITUTION §5)
- Feature control frame model: symbol, tolerance value, zone shape, material and other modifiers, datum references, composite segments, simultaneous-requirement groups (as data; evaluation is elsewhere)
- Governing spec per characteristic: dialect (`ASME` or `ISO`), standard designators with edition, defaults profile reference by ID, version and digest
- Defaults profiles: versioned, content-addressed objects naming default choices per dialect and edition; plan-level and tenant-level default selection with explicit inheritance
- Refusal of characteristics without a governing spec
- ResultRecord assembly (CONSTITUTION §5 minimum fields) and binding of the UNC decision into it
- Characteristic identity (QPId / CharacteristicDesignator) kept stable from plan to result

**Out** (and which node owns it instead):
- Mapping a characteristic to an operator chain, DMT rows, support matrix (owned by `GDT.RES`)
- Computing any value (owned by `GDT.DIM`, `GDT.FORM`, `GDT.LOC`, `GDT.PROF`, `GDT.ZONE`)
- Datum reference frame construction (owned by `GDT.DRF`)
- Creating characteristics from imported PMI (owned by `GDT.PMI`)
- Uncertainty values and decision rules (owned by `UNC`)
- QIF parsing and writing (owned by `IO`)
- Transcription of third-party results (owned by `IO` and `RPT`)

## Requirements

- **REQ-GDT.CHAR-001** Every characteristic shall carry a governing spec naming exactly one dialect (`ASME` or `ISO`), at least one standard designator with edition (for example `std:asme:y14.5:2018`), and one defaults profile referenced by ID, semantic version and SHA-256 digest.
- **REQ-GDT.CHAR-002** If an evaluation command is invoked on a characteristic whose governing spec is missing, incomplete or references an unknown defaults profile, then the system shall reject the command with a typed error naming the missing field and shall emit no ResultRecord.
- **REQ-GDT.CHAR-003** When a plan or tenant default dialect exists, the system shall copy it into a new characteristic's governing spec explicitly at creation time, and the journal entry shall record that the value was inherited and from which level.
- **REQ-GDT.CHAR-004** A defaults profile shall be immutable once published; any change shall produce a new version with a new digest, and existing characteristics shall keep referencing the version they were created with until a journaled command re-points them.
- **REQ-GDT.CHAR-005** The characteristic model shall represent every QIF 3.0 feature-control-frame element listed in Scope (modifiers, composite segments up to the QIF maximum, simultaneous-requirement groups, datum references with modifiers) and shall round-trip them through QIF import and export without loss, verified against the IO round-trip corpus.
- **REQ-GDT.CHAR-006** Every evaluation shall produce exactly one ResultRecord per characteristic per run containing all CONSTITUTION §5 minimum fields, plus the DMT row ID, operator-chain digest and DMT digest supplied by `GDT.RES`.
- **REQ-GDT.CHAR-007** The ResultRecord decision field shall be filled only from a decision returned by a UNC decision rule; if no rule is bound or UNC returns none, then the decision outcome shall be `NOT_DECIDABLE` with a reason code, and GDT shall never compute a conformance outcome itself.
- **REQ-GDT.CHAR-008** The ResultRecord shall record `evaluation_basis` (`POINTS`, `FEATURES`, `RESULTS_ONLY` or `DIRECT_READING`) as declared by the evaluating child and shall never upgrade it.
- **REQ-GDT.CHAR-009** A characteristic's identity (QPId where QIF provides a slot, otherwise the `fx:qpid` extension) shall remain unchanged across edits, re-evaluation and re-export.

## Interfaces

- **Offers:** characteristic and governing-spec types (JSON Schema 2020-12 for MCP output schemas); defaults-profile objects `dp:<dialect>-<edition>@x.y.z#sha256:…`; commands `gdt.create_characteristic@1`, `gdt.set_governing_spec@1`, `gdt.publish_defaults_profile@1` (W2), `gdt.get_result@1` (R0); a ResultRecord builder used by every evaluating child.
- **Consumes:** QIF types and IDs (IO, CORE); UNC decision results and uncertainty statements (UNC); DMT row and chain digest (GDT.RES).

## Decomposition hints

MODEL (QIF characteristic + FCF types), SPEC (governing spec and defaults profiles, inheritance), RECORD (ResultRecord assembly and UNC binding), IDENT (identity and round-trip tests).

## Open questions

- Should a tenant default dialect be allowed at all, or must every plan pick explicitly? The concept docs allow a tenant default shown on every report; confirm with the steering group.
- Who may publish a defaults profile (any contributor, or only reviewers of the DMT)? Linked to the DMT authorship question on `GDT`.

## Split rationale

The dividing principle is **one owner per kind of data**: the faithful QIF mirror (MODEL), identity (IDENT), the per-characteristic rulebook choice (SPEC), the rulebook objects themselves (DPROF), and the output contract (RECORD). Each has a different change rate and a different reviewer: MODEL changes with QIF, DPROF content changes with expert-authored DMT work (D-009), RECORD changes with CONSTITUTION §5.

Alternatives considered:
- **Hint split MODEL / SPEC / RECORD / IDENT (four children).** Adopted with one change: defaults profiles moved out of SPEC into DPROF, because immutability, versioning and agent-branch publication are a separate lifecycle from validating one characteristic's spec.
- **Fold IDENT into MODEL.** Rejected: identity rules are used by export and evaluation paths too, and are small enough to finish as a leaf now while MODEL stays open.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| GDT.CHAR.MODEL | Characteristic and feature-control-frame model on QIF 3.0 | 005 | PCD.DIM.02 |
| GDT.CHAR.IDENT | Stable characteristic identity (leaf) | 009 | none |
| GDT.CHAR.SPEC | Governing spec, dialect selection and refusal (leaf) | 001 002 003 | GOM.GDT.17, PCD.DIM.05, PW.GDT.08 |
| GDT.CHAR.DPROF | Defaults profiles: immutable versioned objects (leaf) | 004 | none |
| GDT.CHAR.RECORD | ResultRecord assembly and decision binding (leaf) | 006 007 008 | none |
