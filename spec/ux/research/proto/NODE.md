---
id: UX.RESEARCH.PROTO
title: Research plan, protocol, consent and data handling
level: 3
parent: UX.RESEARCH
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-design]
satisfies: [REQ-UX.RESEARCH-007, REQ-UX.RESEARCH-008, REQ-UX.RESEARCH-009, REQ-UX.RESEARCH-010]
covers: []
depends_on: []
uses_artifacts: [FLOW.PERSONA.PERS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.RESEARCH.PROTO: Research plan, protocol, consent and data handling

## Purpose

The rules every research activity follows: the published research plan and protocol (so others can repeat it), recruitment criteria tied to FLOW's personas, the informed-consent form, how participant data is stored, anonymised and deleted, the ban on collecting research data through product telemetry without opt-in, and the rule that incumbent workflows are described in our own words with no incumbent screenshots, text or icons. A publication check scans research outputs before they are published.

## Scope

**In:** `design/research/plan.md`, `design/research/protocol.md` (versioned), `design/research/consent.md`, `design/research/data-handling.md`; the participant code scheme; `fid-design research-check`.
**Out:** interview and test sessions (`UX.RESEARCH.INQUIRY`, `UX.RESEARCH.USAB`); SUS reports (`UX.RESEARCH.SUS`); telemetry (OUTR.COMM); the design-partner timing study (FLOW.STUDY).

## Requirements

- **REQ-UX.RESEARCH.PROTO-001** The protocol shall define recruitment criteria per FLOW persona, session scripts, tasks by acceptance-job ID, metrics, and the severity scale reference, and shall carry a version that each study record cites.
- **REQ-UX.RESEARCH.PROTO-002** The consent form shall cover purpose, what is recorded, storage period, who sees raw data, withdrawal at any time, and a separate opt-in for publishing recordings, and no session shall start without a signed or recorded consent referenced by participant code.
- **REQ-UX.RESEARCH.PROTO-003** Published research files shall identify participants only by code `P-<study>-<NN>`, and raw notes, recordings and the code key shall be stored outside the public repository with deletion no later than the period stated in the consent.
- **REQ-UX.RESEARCH.PROTO-004** When `fid-design research-check` runs over files to be published, it shall fail on any e-mail address, phone number, name from the private code key (supplied at run time), raster image without a `capture.json` sidecar naming the project's own capture command, or file listed as an incumbent artefact.
- **REQ-UX.RESEARCH.PROTO-005** The plan shall state that research data is never collected through product telemetry unless the participant opted in to that telemetry (CONSTITUTION C7).

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/research/plan/clean/`, when `fid-design research-check --root clean --key key.csv --json` runs, then it exits 0.
- **AC-2** (error path) Given `has-email/`, when the check runs, then it exits 1 with `RESEARCH-PII email`.
- **AC-3** (error path) Given `has-name/` containing a name listed in the fixture key, when the check runs, then it exits 1 with `RESEARCH-PII name`.
- **AC-4** (error path) Given `screenshot/` (a PNG without capture sidecar), when the check runs, then it exits 1 with `RESEARCH-IMAGE-PROVENANCE`.

## Design

- **Command:** `fid-design research-check --root <dir> [--key <private csv>] [--json]`. Tooling, nearest class R0. The key file is read only from the given path, never written or echoed; findings name the file and line, not the matched name.
- **Errors:** `RESEARCH-PII`, `RESEARCH-IMAGE-PROVENANCE`, `RESEARCH-INCUMBENT`, `RESEARCH-CONSENT-REF`, `RESEARCH-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `research`.

## Test fixtures

`fixtures/ux/research/plan/`: `clean/`, `has-email/`, `has-name/`, `screenshot/`, `key.csv` with invented names; synthetic, CC-BY-4.0.

## Build prompt

> Write `design/research/plan.md`, `protocol.md` (versioned; recruitment per FLOW persona; scripts; tasks by acceptance-job ID; metrics; severity scale reference), `consent.md` (purpose, recordings, storage period, access, withdrawal, separate opt-in to publish recordings) and `data-handling.md` (participant codes `P-<study>-<NN>`, raw data outside the public repository, deletion schedule, no telemetry-based research without opt-in, incumbent workflows described in own words only). In the Rust tooling crate `fid-design`, add `fid-design research-check --root <dir> [--key <csv>] [--json]` failing on e-mail addresses, phone numbers, names from the private key, images without a capture sidecar naming the project's capture command, and files flagged as incumbent artefacts; never print matched names. Exit 0, 1, 2. Tests AC-1..AC-4 of spec node UX.RESEARCH.PROTO.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network; the key is never logged.

## Open questions

- Legal review of the consent form for the jurisdictions where sessions run is [unverified] and belongs to maintainers before the first session.

## Children

_Leaf: none._
