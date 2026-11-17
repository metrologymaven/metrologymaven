---
id: UNC.INST.LIB.CURATE
title: First curated profile set
level: 4
parent: UNC.INST.LIB
kind: feature
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.LIB-001, REQ-UNC.INST.LIB-002]
covers: [SA.UNC.01]
depends_on: [UNC.INST.LIB.FORMAT, UNC.INST.MPE.DERIVE, UNC.INST.MPE.CMM, UNC.INST.MPE.ARM, UNC.INST.MPE.TRK, UNC.INST.MPE.OPT]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.INST.LIB.CURATE: First curated profile set

## Purpose

The content of the first library release: at least 10 profiles for commercially common instruments, each transcribed from a publicly available maker datasheet with URL and retrieval date, derived through the reviewed MPE mapping tables, and checked by a second person. This is content work with source and legal review, so it stays open until the mapping tables are reviewed and counsel has answered the trademark question.

## Scope

**In:**
- Selection of at least 10 instruments (at least one CMM, arm, tracker and scanner)
- Transcription of published MPE values with units, source URL and retrieval date
- Derivation through `unc.derive_instrument_model@1` and a second-person transcription check
- The release `instrument-profile-library@1.0.0`

**Out:**
- Release format and validator (owned by `UNC.INST.LIB.FORMAT`)
- Mapping tables (owned by `UNC.INST.MPE` children)

## Requirements

- **REQ-UNC.INST.LIB.CURATE-001** The first release shall contain at least 10 profiles, with at least one each of device classes CMM, arm, tracker and scanner [unverified: availability of public MPE data].
- **REQ-UNC.INST.LIB.CURATE-002** Every transcribed value shall be checked against its public source by a second person who did not transcribe it, and the check shall be recorded in the release changelog.
- **REQ-UNC.INST.LIB.CURATE-003** Every profile shall be derived only through mapping tables whose review status is `reviewed`, and shall pass `UNC.INST.LIB.FORMAT` validation.
- **REQ-UNC.INST.LIB.CURATE-004** No profile shall contain text copied from a datasheet other than values, units, symbols and the maker and model names.

## Interfaces

- **Offers:** the release `instrument-profile-library@1.0.0`.
- **Consumes:** release format (UNC.INST.LIB.FORMAT); derivation command and mapping tables (UNC.INST.MPE.*).

## Decomposition hints

One content task per device class (CMM, ARM, TRK, OPT) once mapping tables are reviewed.

## Open questions

- Whether maker trademarks and model names may be used in profile IDs needs counsel review (nominative use) [unverified]. If not, profile IDs become neutral and names move into an untrusted display field.
- Whether makers will object to redistribution of transcribed values is unverified; values are facts, but the release should record the source only by URL.

## Children

_None yet: status `open`._
