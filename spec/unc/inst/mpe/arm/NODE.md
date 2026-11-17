---
id: UNC.INST.MPE.ARM
title: Articulated-arm mapping table (ISO 10360-12)
level: 4
parent: UNC.INST.MPE
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.MPE-001, REQ-UNC.INST.MPE-002, REQ-UNC.INST.MPE-005]
covers: [SA.UNC.01]
depends_on: [UNC.INST.MPE.DERIVE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.MPE.ARM: Articulated-arm mapping table (ISO 10360-12)

## Purpose

The reviewed `mpe-map/v1` table for portable articulated-arm CMMs with contact probes: articulated-arm CMM terms (probing size and form, length, articulated location) from ISO 10360-12 [unverified: term set]. For each term it records the symbol, the clause, the formula shape, the contributor it maps to, the distribution assumption and the task applicability limits. The content must be checked against the purchased standard text by an instrument-metrology expert, so this node is content work and stays open until an expert author is assigned.

## Scope

**In:**
- The table file(s) `mpe-map/<part>@<semver>` in `mpe-map/v1` format
- Term-to-contributor choices and distribution assumptions with a one-line rationale each (≤ 280 characters, no standards text)
- Task applicability limits per term (which measurement tasks the MPE is a sound basis for)
- Review record with the edition held (D-016)

**Out:**
- Table format, lint and derivation command (owned by `UNC.INST.MPE.DERIVE`)
- Other instrument classes (owned by the sibling `UNC.INST.MPE` children)
- Profiles that use the table (owned by `UNC.INST.LIB.CURATE`)

## Requirements

- **REQ-UNC.INST.MPE.ARM-001** The table shall map every MPE term of the cited part that a maker datasheet typically states to exactly one contributor, or mark it `not_mapped` with a reason [unverified: typical datasheet term set].
- **REQ-UNC.INST.MPE.ARM-002** Every term shall state its distribution assumption and task applicability, and every row shall cite designator, edition and clause only.
- **REQ-UNC.INST.MPE.ARM-003** The table shall pass the `UNC.INST.MPE.DERIVE` lint and carry review status `reviewed` by an instrument-metrology expert who is not its author, with the edition held recorded, before any profile is derived from it.

## Interfaces

- **Offers:** the mapping table for the part(s) named in the title.
- **Consumes:** `mpe-map/v1` format, lint and derivation (UNC.INST.MPE.DERIVE).

## Decomposition hints

One content task per part once an expert author is assigned; a worked example datasheet (invented values) for the derivation tests.

## Open questions

- Term symbols, their formulas and the clause numbers must be checked against the purchased text of the cited part(s) [unverified].
- Whether MPE terms for tasks other than the one the test defines (for example form evaluation from a length MPE) may be used at all, or only with a disclosed applicability flag, needs the expert's judgement.

## Children

_None yet: status `open`._
