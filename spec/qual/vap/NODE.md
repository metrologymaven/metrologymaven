---
id: QUAL.VAP
title: Validation Accelerator Pack documents and templates
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-QUAL-001]
covers: []
depends_on: [QUAL.IQOQ.RECORD, CORE.BUS.POL.FS, CORE.REG.DEF]
uses_artifacts: [QUAL.EVID.LAYOUT, TRUST.PROFILE.MAP, QUAL.MATRIX.DATA, QUAL.VULN.ADVIS, CORE.REG.MAN]
cli: [fid validate generate-pack, fid validate make-review-report]
mcp: [validate_generate_pack, validate_make_review_report]
journal: [validate.generate_pack@1, validate.make_review_report@1]
updated: 2026-11-17
---

# QUAL.VAP: Validation Accelerator Pack documents and templates

## Purpose

The document half of the Validation Accelerator Pack: the templates a regulated customer fills in to validate the software for their intended use, generated per release and per customer configuration. It covers the user requirements template, the risk assessment, the IQ, OQ and PQ protocol templates, framework checklists, the supplier questionnaire and the periodic-review report. The customer owns validation; this node supplies honest, pre-filled starting points that point at the evidence other nodes produce.

## Scope

**In:**
- URS template pre-filled from intended-use selections (for example scan inspection, CMM evaluation, transcription reports)
- Risk assessment template in the style of FDA CSA guidance: intended use to feature to process risk, scripted versus unscripted testing [unverified guidance details]
- IQ and OQ protocol templates whose steps and acceptance criteria name the `QUAL.IQOQ` commands and record fields
- PQ guidance: a template for the customer's own calibrated-part study (for example an ISO 15530-3 style workflow), never automated
- Framework checklists (17025 records, Part 11 and Annex 11) that link to rows of the compliance matrix and to `TRUST.PROFILE.MAP`, by clause ID only
- GAMP 5 categorisation rationale [unverified category] and supplier-questionnaire answers that state gaps plainly (for example no ISO 9001 certification of the project)
- `validate.generate_pack@1`: render the pack for a release and a customer configuration into an approved export root
- `validate.make_review_report@1`: periodic-review report from journaled qualification records, changes and open advisories over a date range

**Out** (and which node owns it instead):
- Running IQ and OQ (owned by `QUAL.IQOQ`)
- The per-release evidence bundle (owned by `QUAL.EVID`)
- The compliance matrix itself (owned by `QUAL.MATRIX`)
- Part 11 control mapping of the Regulated profile (owned by `TRUST.PROFILE.MAP`)
- Validation services or any statement that the customer's system is validated (not offered)

## Requirements

- **REQ-QUAL.VAP-001** Each release shall publish a VAP template set containing at least a URS template, a risk assessment template, IQ, OQ and PQ protocol templates, a 17025 checklist, a Part 11 and Annex 11 checklist and a supplier questionnaire.
- **REQ-QUAL.VAP-002** Every IQ and OQ protocol step shall name the registry command it runs and the qualification-record field that holds its acceptance result.
- **REQ-QUAL.VAP-003** When `validate.generate_pack@1` runs with a release version and an intended-use selection, the system shall write the pack into an approved export root with the release version, evidence-bundle digest and selection recorded on every document.
- **REQ-QUAL.VAP-004** Every checklist row shall cite its framework by designator, edition and clause ID only, and shall contain no quoted standard or regulation text.
- **REQ-QUAL.VAP-005** Every generated document shall carry a statement that validation and compliance decisions belong to the customer.
- **REQ-QUAL.VAP-006** The supplier questionnaire answers shall state each unmet expectation as a gap with a reason, and shall not mark any item met without a link to evidence.
- **REQ-QUAL.VAP-007** When `validate.make_review_report@1` runs for a date range, the system shall list every qualification record, version change and security advisory affecting the installed version in that range.
- **REQ-QUAL.VAP-008** Templates shall be published in an editable open format and a machine-readable JSON form, under a licence that lets customers modify and reuse them.

## Interfaces

- **Offers:** `validate.generate_pack@1` (W1, `fs_effect: write_root`), `validate.make_review_report@1` (W1, `fs_effect: write_root`); VAP template set per release.
- **Consumes (code via `depends_on`, files via `uses_artifacts`):** the record type and queries from `QUAL.IQOQ.RECORD`; the Registry Manifest (`CORE.REG.MAN`) and record schema for protocol checks; evidence-bundle digest from `QUAL.EVID`; `TRUST.PROFILE.MAP` rows for Part 11; matrix row IDs from `QUAL.MATRIX` as published data (not a build dependency); advisory list from `QUAL.VULN` as published data.

## Decomposition hints

URS, RISK, PROTO (IQ/OQ/PQ protocol templates), CHECK (framework checklists), SUPQ (supplier questionnaire and GAMP rationale), GEN (pack generator command), REVIEW (periodic-review report).

## Split rationale

The split separates the template format and framing templates (TPL) from the three document families that each have their own correctness check (PROTO against the registry, CHECK against the matrix and profile map, SUPQ against the evidence bundle), and from the two product commands that render them for a customer (GEN) and summarise qualification history (REVIEW). Template children are documents plus a repository check (Tooling); GEN and REVIEW are registry commands in `fid-qual`.

Alternatives considered:
- **URS and RISK as separate leaves** (the hints). Rejected: both are plain templates with no check beyond the shared schema; together with PQ guidance they fill one leaf.
- **One generator leaf for GEN and REVIEW.** Rejected: REVIEW reads journal records and advisory feeds, GEN only templates; their inputs and errors differ.

## Open questions

- **Revise for D-036/D-037 (accepted 2026-11-17):** templates stay open in every release; the paid offering is the certified build with a completed dossier and support (D-036 rule 3 clarification).
- Template licence: CC-BY-4.0 follows D-001's documentation licence; CC0 remains a maintainer option.
- Which intended uses the P1a pack covers: the catalogue starts with transcription reports, the prismatic GD&T subset and scan inspection, matching launch-1.
- The concept places VAP v1 in P1b; this node keeps P1a because REQ-QUAL-001 applies to every release. A phase move needs a decision.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.VAP.TPL | Template format, URS, risk assessment and PQ guidance templates | 001 008 | none |
| QUAL.VAP.PROTO | IQ and OQ protocol templates and their command check | 001 002 | none |
| QUAL.VAP.CHECK | Framework checklists (17025, Part 11 and Annex 11) | 001 004 | none |
| QUAL.VAP.SUPQ | Supplier questionnaire and GAMP categorisation rationale | 001 006 | none |
| QUAL.VAP.GEN | Validation pack generator command | 003 005 | none |
| QUAL.VAP.REVIEW | Periodic-review report command | 007 | none |
