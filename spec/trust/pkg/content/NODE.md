---
id: TRUST.PKG.CONTENT
title: Package content selection and evidence tiers
level: 3
parent: TRUST.PKG
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.PKG-003, REQ-TRUST.PKG-004, REQ-TRUST.PKG-005, REQ-TRUST.PKG-006]
covers: []
depends_on: [TRUST.PKG.MANIFEST]
uses_artifacts: [CORE.JRN.ENTRY, KRN.MAN.MANI.SCHEMA]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PKG.CONTENT: Package content selection and evidence tiers

## Purpose

Decides **what goes into** a Result Package for a given export request: which ResultRecords, the QIF Results payload, the evidence tier (results only, features, or full points), every input point set the included results were computed from, the journal segment from the nearest checkpoint to the last included entry, the kernel manifest and the active profile. It produces an in-memory content set that `TRUST.PKG.MANIFEST` serialises. It is separate because it is the only part of the package work that reaches into CORE, IO and KRN, and it carries the completeness rules a verifier relies on.

## Scope

**In:**
- Resolving an export selection (run, plan or explicit result list) into ResultRecords
- Completeness check of the CONSTITUTION §5 minimum fields on every record
- Evidence tier choice and its mapping to `evaluation_basis`
- Gathering input point sets by input digest, with deduplication
- Choosing the journal segment and checkpoint(s) that cover the included entries
- UEC-0 handling ("not evaluated", never a substituted value)
- Including the kernel manifest and profile document as members

**Out** (and which node owns it instead):
- Byte layout and serialisation (`TRUST.PKG.MANIFEST`)
- The export command and export-root handling (`TRUST.PKG.EXPORT`)
- Producing QIF Results bytes (`IO.QIF`)
- Journal storage and checkpoint creation (`CORE.JRN`)
- Profile definitions (`TRUST.PROFILE.DEFS`)

## Requirements

- **REQ-TRUST.PKG.CONTENT-001** If any selected ResultRecord lacks a CONSTITUTION §5 minimum field, then content assembly shall fail with error `pkg.record_incomplete` naming the record ID and every missing field.
- **REQ-TRUST.PKG.CONTENT-002** When the evidence tier is `points`, content assembly shall include every point set whose digest appears in an included ResultRecord's input digests, and shall fail with `pkg.points_missing` if one cannot be found in the project store.
- **REQ-TRUST.PKG.CONTENT-003** When the evidence tier is `features` or `results`, content assembly shall include no point set and shall record the tier in the manifest.
- **REQ-TRUST.PKG.CONTENT-004** Content assembly shall include the journal entries from the nearest checkpoint at or before the first included entry through the last entry referenced by any included ResultRecord, with no gaps.
- **REQ-TRUST.PKG.CONTENT-005** If the journal has no checkpoint at or before the first included entry, then content assembly shall include the segment from the session header entry.
- **REQ-TRUST.PKG.CONTENT-006** When a selected ResultRecord has uncertainty class UEC-0, content assembly shall carry it with uncertainty marked "not evaluated" and no numeric uncertainty value.
- **REQ-TRUST.PKG.CONTENT-007** Content assembly shall include each distinct point set once, keyed by its SHA-256, however many results reference it.
- **REQ-TRUST.PKG.CONTENT-008** Content assembly shall exclude detachable journal fields that were redacted in the source journal and shall keep their salted commitments so the chain still verifies.

## Interfaces

- **Offers:** `assemble(selection, tier) -> ContentSet` used by `TRUST.PKG.EXPORT`.
- **Consumes:** ResultRecords, point-set blobs and journal entries from `CORE`; QIF Results bytes from `IO.QIF`; UEC status as carried in each ResultRecord (no direct UNC dependency, to avoid a cycle through `UNC.ORS` and `GDT.RES`); kernel manifest from `KRN.MAN`; active profile document from `TRUST.PROFILE.DEFS`.

## Decomposition hints

SELECT (selection to records, completeness), EVID (tiers and point gathering), JSEG (journal segment and checkpoint choice), plus a small UEC0 rule that could fold into SELECT.

## Open questions

- REQ-TRUST.PKG.CONTENT-008 assumes CORE.JRN's detachable-field commitments let a segment be shipped without the detached bodies. CORE.JRN.ENTRY now specifies header/payload separation with salted commitments (128-bit salt, SHA-256 over salt then canonical bytes), which is what `TRUST.PKG.CONTENT.JSEG` relies on.
- Very large point sets may need chunked members; a chunking rule would be a minor format version.
- Where ResultRecords are stored in the project (which CORE or GDT leaf owns the record table) is not yet stated by CORE; the children read them through the bus `StateView` (`CORE.BUS.PIPE.HANDLER`) [unverified that the view exposes them].
- D-020 lets a package carry its own kernel artifact. Nothing here adds it yet; an optional `include_kernel` flag would be a later minor change (see `TRUST.REPLAY.KART`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.PKG.CONTENT.SELECT | Selection resolution, record completeness and content assembly | 001 006 | none |
| TRUST.PKG.CONTENT.EVID | Evidence tiers and point-set gathering | 002 003 007 | none |
| TRUST.PKG.CONTENT.JSEG | Journal segment and checkpoint choice | 004 005 008 | none |
