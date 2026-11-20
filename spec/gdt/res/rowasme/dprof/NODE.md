---
id: GDT.RES.ROWASME.DPROF
title: ASME Y14.5-2018 defaults profile values
level: 4
parent: GDT.RES.ROWASME
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
needs_expert: gdt
satisfies: [REQ-GDT.RES.ROWASME-001, REQ-GDT.RES.ROWASME-002, REQ-GDT.RES.ROWASME-004]
covers: []
depends_on: [GDT.RES.ROWASME.INV, GDT.RES.ROWFLOW.REVIEW, GDT.CHAR.DPROF]
uses_artifacts: [GDT.RES.SCHEMA]
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# GDT.RES.ROWASME.DPROF: ASME Y14.5-2018 defaults profile values

## Purpose

The values of the defaults profile `dp:asme-y14.5-2018@1.x`: for each key in the schema's defaults-profile vocabulary, which variant or setting applies when a drawing invokes Y14.5-2018 and says nothing more (for example the default position variant, or the default decision-rule hint passed to UNC). The keys are structure (owned by `GDT.RES.SCHEMA` and `GDT.CHAR.DPROF`); the values are interpretation, so they are authored and reviewed exactly like rows.

## Scope

**In:**
- One value per defaults-profile key used by any P1a ASME row
- A profile version tied to the table version it was reviewed with
- Review through the same two-reviewer, edition-attested process as rows

**Out** (and which node owns it instead):
- The key vocabulary and profile format (owned by `GDT.RES.SCHEMA`, `GDT.CHAR.DPROF`)
- Rows (owned by the family children `GDT.RES.ROWASME.SIZE`, `FORM`, `ORIENT`, `POS`, `DRF`)
- Decision rules (owned by `UNC`)

## Requirements

- **REQ-GDT.RES.ROWASME.DPROF-001** The profile shall give a value for every defaults-profile key that any row in the P1a ASME table reads, and every value naming a variant shall name a variant that row defines.
- **REQ-GDT.RES.ROWASME.DPROF-002** The profile values shall be authored by a human with roster basis `paid_author` or `partner` and signed off by two distinct reviewers with edition-held attestations, recorded as for rows.
- **REQ-GDT.RES.ROWASME.DPROF-003** The profile shall carry the table version and digest it was reviewed against, and the release gate shall fail if they differ from the released table.
- **REQ-GDT.RES.ROWASME.DPROF-004** No profile value or note shall contain reproduced standards text.

## Interfaces

- **Offers:** `dp:asme-y14.5-2018@1.x` values, loaded by `GDT.CHAR.DPROF` and used by the resolver for variant selection.
- **Consumes:** key vocabulary (GDT.RES.SCHEMA, read as a format); profile format (GDT.CHAR.DPROF); review command (GDT.RES.ROWFLOW.REVIEW).

## Decomposition hints

None for agents.

## Open questions

- **D-045 (accepted 2026-11-19):** profiles count as interpretation rules; until the profile sign-off path above is settled, `GDT.RES.STATUS` reports every profile `provisional` (`no_signoff_path`), so every result is labelled `interpretation: provisional`. Settling the sign-off key is therefore on the critical path of the first-review subset.
- Profiles are not DMT rows, so `gdt.review_dmt_row@1` cannot review them as written. Either profiles become a row-like record that the ROWFLOW state machine accepts (a key of the form `dmt:asme/defaults_profile/none@v`) or `GDT.CHAR.DPROF` gains its own review step. Proposed: the row-like key, so one workflow and one gate cover both; confirm with the `GDT.CHAR.DPROF` maintainers.
- REQ-003's gate check is not in `GDT.RES.ROWASME.GATE` yet; add it when the profile representation above is settled.
- Blocked on people: the paid author(s) and licences (D-009, D-016).

## Children

_None yet: status `open`._
