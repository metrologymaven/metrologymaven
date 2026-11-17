---
id: IO.DMIS.DIAL
title: Vendor DMIS dialect profiles as data
level: 3
parent: IO.DMIS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-007]
covers: []
depends_on: [IO.DMIS.PARSE]
cli: [fid import dmis-list-dialects]
mcp: [import_dmis_list_dialects]
journal: [import.dmis_list_dialects@1]
updated: 2026-11-16
---

# IO.DMIS.DIAL: Vendor DMIS dialect profiles

## Purpose

Describes vendor deviations from DMIS 5.3 (extra statements, relaxed syntax, vendor output formats) as versioned data files rather than code forks, so the parser can accept a vendor's files when a profile is selected and the journal records which profile was used.

## Scope

**In:**
- The profile format: ID, version, base DMIS version, extra keywords and their argument shapes, relaxed lexical rules
- Profile validation and listing (`import.dmis_list_dialects@1`, R0)
- Recording profile ID and version in import provenance
- Clean-room profiles written from public documentation and observed files only (CONSTITUTION §8)

**Out:**
- The grammar (`IO.DMIS.PARSE`), which offers the hook

## Requirements

- **REQ-IO.DMIS.DIAL-001** When a dialect profile is selected, the parser shall accept exactly that profile's documented deviations and no others.
- **REQ-IO.DMIS.DIAL-002** The profile ID and version shall be recorded in the journal of every import that used it.
- **REQ-IO.DMIS.DIAL-003** If a profile file fails validation against the profile schema, then loading shall fail with `dialect_invalid` and the profile shall not be listed.
- **REQ-IO.DMIS.DIAL-004** The system shall expose an R0 command listing installed dialect profiles with ID, version and base DMIS version.

## Interfaces

- **Offers:** profile schema, `import.dmis_list_dialects@1` (R0), `Dialect` value passed to `IO.DMIS.PARSE`.
- **Consumes:** `IO.DMIS.PARSE` hook.

## Decomposition hints

SCHEMA, LOAD, LIST, and one sample profile per incumbent built from public documentation [unverified: availability].

## Open questions

- Installing third-party profiles: data, not code, so maybe not A0; confirm against D-007's plugin rule.

## Children

_None yet: status `open`._
