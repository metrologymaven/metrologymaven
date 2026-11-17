---
id: FLOW.STUDY
title: Design-partner timing and usability study protocol
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-FLOW-003, REQ-FLOW-007]
covers: [JOB.USE.02, JOB.USE.03]
depends_on: [FLOW.PERSONA, CORE.JRN, OUTR.BENCH, OUTR.BRAND]
cli: [fid accept import-timing, fid accept show-study]
mcp: [accept_show_study]
journal: [accept.import_timing@1]
updated: 2026-11-16
---

# FLOW.STUDY: Design-partner timing and usability study protocol

## Purpose

Owns the published protocol by which the project measures real task time and usability with design partners, and the anonymised records those sessions produce. The nightly suite measures machine time and correctness on public data; only partners at their own sites, on their own data, can measure human task time, first-time success and System Usability Scale scores. STUDY defines how sessions are run, what is recorded, how partner data stays on site, and what evidence any public comparative claim needs.

## Scope

**In:**
- The timing-study protocol: 3 to 5 partners, two sessions each, stopwatch plus journal timestamps, task time from file open to exported released report
- Incumbent baseline: the partner times its own licensed tool on the same data
- Usability measures: unassisted first-time success on J1 to J3 after one tutorial (`JOB.USE.02`), System Usability Scale (`JOB.USE.03`)
- The anonymised timing record format and its import, consumed by the gate
- The claim rule: what evidence a public performance claim needs

**Out** (and which node owns it instead):
- Gate evaluation using these records (owned by `FLOW.RUNNER`)
- Usability research methods beyond these measures (owned by `UX`)
- Publishing benchmarks and comparisons (owned by `OUTR.BENCH`; incumbent comparisons are D-021)
- Claim wording rules for the website (owned by `OUTR.BRAND`)
- Incumbent result correlation (owned by `MIG`)

## Requirements

- **REQ-FLOW.STUDY-001** The project shall publish a timing-study protocol that defines partner count (at least 3), sessions per partner (at least 2), task start and end events, the incumbent-baseline procedure and the usability measures.
- **REQ-FLOW.STUDY-002** Task time in a study shall run from the journaled file-open event to the journaled export of the released report, and the record shall carry both the journal timestamps and the observer's stopwatch time.
- **REQ-FLOW.STUDY-003** A study record shall contain only job ID, release, anonymous partner token, governance profile, times, success flag, SUS score and assistance flag, and shall contain no part data, drawings or partner identity.
- **REQ-FLOW.STUDY-004** Partner data shall stay at the partner site; if a study record or attachment contains anything beyond the fields of REQ-FLOW.STUDY-003, then `fid accept import-timing` shall reject it.
- **REQ-FLOW.STUDY-005** The protocol shall measure unassisted first-time success on J1, J2 and J3 after one tutorial against the ≥ 80 % bar [unverified], and SUS against the ≥ 70 bar [unverified].
- **REQ-FLOW.STUDY-006** Incumbent baseline times shall be stored only as internal study records, and the project shall make no public comparative performance claim unless the claim cites a published study summary meeting this protocol and the conditions set by D-021.
- **REQ-FLOW.STUDY-007** Importing a study record shall be a human-only journaled command (`accept.import_timing@1`, W2) and shall not be exposed over MCP.

## Interfaces

- **Offers:** the protocol document; the study record format; `accept.import_timing@1` (W2, human-only); `accept.show_study@1` (R0, aggregates only); median and maximum task time per job per release for `FLOW.RUNNER`.
- **Consumes:** journal timestamps (`CORE.JRN`); persona and job maps (`FLOW.PERSONA`); benchmark publication rules (`OUTR.BENCH`, `OUTR.BRAND`).

## Decomposition hints

PROTO (protocol document), RECORD (record format, anonymisation and import), USAB (first-time success and SUS), CLAIM (public-claim evidence rule).

## Open questions

- Incumbent licence terms may forbid publishing benchmark numbers [unverified]; the publishing side is D-021 (open). This node defines only the evidence.
- Opt-in crash and performance telemetry from pilot installs is a third measurement source in doc 10; it is consumed by `FLOW.RUNNER`, not here.

## Children

_None yet: status `open`._
