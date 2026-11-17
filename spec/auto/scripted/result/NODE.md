---
id: AUTO.SCRIPTED.RESULT
title: ResultRecord labelling and the value-of-record rule
level: 3
parent: AUTO.SCRIPTED
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO.SCRIPTED-003, REQ-AUTO.SCRIPTED-004, REQ-AUTO.SCRIPTED-007]
covers: [GOM.AUT.04]
depends_on: [AUTO.SCRIPTED.DECL, AUTO.REC.LIFE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SCRIPTED.RESULT: ResultRecord labelling and the value-of-record rule

## Purpose

Keeps scripted checks honest. Every ResultRecord a scripted check yields carries the script's content digest and version next to the algorithm ID fields; every value of record in it must be traceable to a kernel command result (by journal entry hash and output path); and a result can be released only if the script version on it is released.

## Scope

**In:**
- ResultRecord fields for script identity (digest, version, implementation kind)
- Provenance check: each value of record must cite a kernel command result in the same recompute or an earlier committed entry
- Refusal of script-supplied values as values of record; such values may be stored as annotations only
- Release check on results of scripted checks

**Out:**
- ResultRecord core schema (`CORE`, `RPT.MODEL`), decision rules (`UNC.DEC`)
- Kernel formula command for derived values (D-027, `KRN`)

## Requirements

- **REQ-AUTO.SCRIPTED.RESULT-001** Every ResultRecord from a scripted check shall carry the implementation's content digest, version and kind.
- **REQ-AUTO.SCRIPTED.RESULT-002** Each value of record in such a ResultRecord shall cite the journal entry hash and output field of the kernel command result it came from, and if a value cites none, then the system shall refuse to record it as a value of record.
- **REQ-AUTO.SCRIPTED.RESULT-003** If a cited value differs from the cited command output, then the system shall refuse the whole ResultRecord with `provenance_mismatch`.
- **REQ-AUTO.SCRIPTED.RESULT-004** If a release of results includes a result whose script version is not in the released state, then that release shall be refused naming the result and script.

## Interfaces

- **Offers:** labelling and provenance check used by `AUTO.SCRIPTED.GRAPH` on output; release precondition used by `TRUST`/`RPT` release commands.
- **Consumes:** ResultRecord schema; `AUTO.REC.LIFE` lifecycle query; CORE journal.

## Decomposition hints

LABEL (identity fields), PROV (provenance check), RELEASE (release precondition).

## Open questions

- D-027 decides whether a kernel formula command exists; if it does, "ratio of two distances" becomes a kernel result a scripted check can cite. Until then, scripted checks can only orchestrate and select kernel results.
- Which node owns the release command for results (`RPT` or `TRUST`) determines where REQ-AUTO.SCRIPTED.RESULT-004's precondition is declared.

## Children

_None yet: status `open`._
