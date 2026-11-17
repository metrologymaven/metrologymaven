---
id: MIG.SRC
title: Incumbent source register and clean-room admission
level: 2
parent: MIG
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-MIG-007]
covers: []
depends_on: [IO.REG.DETECT, IO.REG.ENV, CORE.JRN.CANON]
uses_artifacts: [CORE.REG.MAN, ECO.PARITY.GEN]
cli: [fid migrate list-sources, fid migrate check-source]
mcp: [migrate_list_sources, migrate_check_source]
journal: [migrate.list_sources@1, migrate.check_source@1]
updated: 2026-11-17
---

# MIG.SRC: Incumbent source register and clean-room admission

## Purpose

The single place that says which incumbent artefacts migration tooling may read, and by which route. Each incumbent format (results files, point exports, project exports, report forms, program files) gets a register entry stating its access route: a public specification, a user-exported open format (QIF, DMIS, CSV, AQDEF, STEP), or "not admitted". An entry is admitted only with recorded evidence that reading it needs no copied code, no reverse engineering forbidden by a licence, and no incumbent text or assets. Every other MIG child reads incumbent data only through admitted entries, so REQ-MIG-007 is enforced in one place.

## Scope

**In:**
- The source register as versioned data: incumbent product, artefact kind, format name, version range, access route, admission status (`ADMITTED`, `OPEN_EXPORT_ONLY`, `NOT_ADMITTED`, `PENDING_REVIEW`), evidence link, reviewer and date
- The admission rule and review checklist (clean-room provenance, EULA review note, no copied strings or assets)
- The "export to open format" route for each non-admitted artefact: which open format the user should export to, documented in our own words
- A gate that MIG import and conversion commands call before reading, refusing non-admitted sources
- `migrate.list_sources@1` (R0), `migrate.check_source@1` (R1)

**Out** (and which node owns it instead):
- Parsing of any format (owned by `IO`)
- The decision on which incumbent program and project formats are admissible (D-033, accepted; this node records outcomes)
- Copy detection of incumbent text in our content (owned by `OUTR.BRAND` and D-013)
- Use of the sources in each ladder stage (owned by `MIG.TEMPL`, `MIG.PROJ`, `MIG.PROGEQ`, `MIG.PARALLEL`)

## Requirements

- **REQ-MIG.SRC-001** The project shall keep a source register in which every incumbent artefact that any MIG command reads has exactly one entry with an access route and an admission status.
- **REQ-MIG.SRC-002** An entry shall be set to `ADMITTED` only when it links a review record naming the public specification or user-exported open format relied on, the reviewer, the date and a statement that no incumbent code, text or assets were used.
- **REQ-MIG.SRC-003** If a MIG import or conversion command is asked to read an artefact whose register entry is not `ADMITTED` or `OPEN_EXPORT_ONLY`, or that has no entry, then the command shall refuse with `source_not_admitted` before reading any bytes beyond format detection, and shall name the open-format export route if one is registered.
- **REQ-MIG.SRC-004** The register check shall fail the build if any MIG command declares an input format that has no register entry.
- **REQ-MIG.SRC-005** `migrate.list_sources@1` shall return every register entry with its status and export route, and the same data shall generate the public sources page with no hand edits.
- **REQ-MIG.SRC-006** Register entries and export-route notes shall contain no text copied from incumbent documentation; incumbent product names shall be used only to identify the products.

## Interfaces

- **Offers:** `migrate.list_sources@1` (R0), `migrate.check_source@1` (R1), `SourceRegister` data (`mig-sources@1`, with version), `source_gate(format_id, source_id?)` check used by MIG import and conversion commands, the generated sources page (`mig-sources.md` with the `ECO.PARITY.GEN` header).
- **Consumes (code):** `IO.REG.DETECT` format IDs and detection; `IO.REG.ENV` import envelope (the gate runs inside it for MIG commands); `CORE.JRN.CANON`.
- **Consumes (artifacts):** `CORE.REG.MAN` registry manifest for the build check; `ECO.PARITY.GEN` header format for the generated page.

## Decomposition hints

REG (register schema and data), GATE (refusal gate and build check), PAGE (generated sources page).

## Open questions

- D-033 is accepted: option A (open exports only) through P2, then per-format counsel review (option B) at P4. Until then every incumbent-native program and project format is `NOT_ADMITTED` or `PENDING_REVIEW`; `MIG.SRC.REG` enforces this with a register policy field. D-033's condition (counsel review before the node is published or released) applies to the register's first release.

## Split rationale

Split by **who reads the register and when**: the register itself, its listing and its generated page (REG: data plus an R0 read, all describing the same entries), and the enforcement path (GATE: the runtime refusal every MIG reader calls, plus the build check that every MIG input format is registered). PAGE is folded into REG because the page is a rendering of the same `list_sources` output.

Alternatives considered:
- **Leaf.** Rejected: data schema with review records, a runtime gate wired into the IO envelope, a build check and two commands exceed one M PR.
- **Gate inside `IO.REG.ENV`.** Rejected: the admission policy is a MIG rule (REQ-MIG-007); IO stays policy-free and only offers the hook.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [MIG.SRC.REG](reg/NODE.md) | Source register, review records, listing and sources page (leaf) | 001 002 005 006 | none |
| [MIG.SRC.GATE](gate/NODE.md) | Admission gate, check command and register build check (leaf) | 003 004 | none |
