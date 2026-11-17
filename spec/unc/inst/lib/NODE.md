---
id: UNC.INST.LIB
title: Instrument-profile library
level: 3
parent: UNC.INST
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-008, REQ-UNC.INST-002]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, UNC.INST.MPE]
cli: [fid unc list-profile]
mcp: [unc_list_profile]
journal: [unc.list_profile@1]
updated: 2026-11-16
---

# UNC.INST.LIB: Instrument-profile library

## Purpose

A curated, versioned library of specification-derived instrument models for common CMMs, arms, trackers and scanners, each built from publicly published MPE values and citing its public source. It is offered at import so a first result can be UEC-1 instead of UEC-0. Curation is content work with source verification, so the node stays open until the first profile set is gathered.

## Scope

**In:**
- Library release format: a set of `instrument-model/v1` files with a release digest
- Curation rules: public source only (maker datasheet URL and retrieval date), no copied text, values transcribed with units
- `unc.list_profile@1` (R0) with filters by device class and maker
- First release of at least 10 profiles

**Out:**
- Mapping tables (owned by `UNC.INST.MPE`)
- Maker-signed IEMPS (owned by `UNC.INST.IEMPS`)

## Requirements

- **REQ-UNC.INST.LIB-001** The first library release shall contain at least 10 profiles covering at least one CMM, arm, tracker and scanner each, every one citing a public source with URL and retrieval date [unverified: availability of public MPE data].
- **REQ-UNC.INST.LIB-002** Every profile shall be derived through `UNC.INST.MPE` mappings and shall be kind `spec_derived` (UEC-1).
- **REQ-UNC.INST.LIB-003** When `unc.list_profile@1` is called, the system shall return profile IDs, device class, maker, model name, source and digest, filterable by class and maker.
- **REQ-UNC.INST.LIB-004** If a profile's source becomes unavailable or is corrected, then the library shall publish a new version and keep the old profile resolvable by digest.

## Interfaces

- **Offers:** profile library releases; `unc.list_profile@1`.
- **Consumes:** model format (UNC.INST.SCHEMA); mappings (UNC.INST.MPE).

## Decomposition hints

FORMAT (release format and listing command, likely a leaf), CURATE (first profile set, a content task with source review).

## Open questions

- Whether maker trademarks and model names may be used in profile IDs needs counsel review (nominative use) [unverified].

## Children

_None yet: status `open`._
