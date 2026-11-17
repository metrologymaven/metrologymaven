---
id: CMM.SIM.REPORT
title: Simulation report model, clean rule and digest
level: 3
parent: CMM.SIM
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.SIM-002, REQ-CMM.SIM-005]
covers: []
depends_on: [CORE.STO]
side_effect: R0
determinism: D0
cli: [fid cmm get-simulation-report]
mcp: [cmm_get_simulation_report]
journal: [cmm.get_simulation_report@1]
updated: 2026-11-16
---

# CMM.SIM.REPORT: Simulation report model, clean rule and digest

## Purpose

Defines the simulation report, the artifact the Motion Executive's simulation gate binds to: plan digest, machine, fixture, part-placement and probe-configuration digests, simulator identity and fidelity level, per-move verdicts, unintended contacts and cycle-time estimate. It builds the report from simulation outputs, applies the clean rule, stores it under its own content digest and serves it read-only.

## Scope

**In:**
- `SimulationReport` type and canonical form
- `build_report(inputs) -> SimulationReport` (pure) called by `CMM.SIM.RUN`
- Clean rule
- Content-addressed storage of reports and `cmm.get_simulation_report@1` (R0)

**Out** (and which node owns it instead):
- Running the simulation (owned by `CMM.SIM.RUN`)
- Finding a clean report for a tuple (owned by `CMM.SIM.GATE`)
- Verdict computation (owned by `CMM.COLL.CHECK`)

## Requirements

- **REQ-CMM.SIM.REPORT-001** The system shall build a simulation report containing plan digest, machine-model, fixture, part-placement and probe-configuration digests, simulator identity, version and fidelity level, one verdict per move, the list of unintended contacts, the timing model parameters and the estimated cycle time in seconds.
- **REQ-CMM.SIM.REPORT-002** The system shall mark a report `clean: true` only if every move's verdict is `CLEAR` and the unintended-contact list is empty; any `COLLISION`, `MARGIN` or `UNCHECKED` verdict shall make it `clean: false`.
- **REQ-CMM.SIM.REPORT-003** If the number of verdicts differs from the number of moves in the plan, then the system shall refuse to build the report with `REPORT_INCOMPLETE`.
- **REQ-CMM.SIM.REPORT-004** The system shall store each report under SHA-256 of its RFC 8785 canonical JSON and shall return that digest.
- **REQ-CMM.SIM.REPORT-005** When a report is requested by digest, the system shall return it and verify that its content still hashes to the digest, failing with `REPORT_CORRUPT` otherwise.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/sim/report_inputs_clean.json` (12-move plan, all `CLEAR`), when the report is built and stored, then `clean` is true and the digest equals the fixture's `expected_digest`.
- **AC-2** Given fixture `fixtures/cmm/sim/report_inputs_margin.json` (move 7 `MARGIN`), when built, then `clean` is false.
- **AC-3** Given fixture `fixtures/cmm/sim/report_inputs_unchecked.json` (no fixture model, moves 3 to 5 `UNCHECKED`), when built, then `clean` is false.
- **AC-4** (error path) Given 11 verdicts for a 12-move plan, when built, then it fails with `REPORT_INCOMPLETE`.
- **AC-5** Given the AC-1 digest, when `fid cmm get-simulation-report --digest <d> --json` runs, then the stored report is returned; with one byte of the stored blob altered, it fails with `REPORT_CORRUPT`.
- **AC-6** (replay) Given AC-1 built twice, then the two reports are byte-identical.

## Design

- **Registry command:** `cmm.get_simulation_report@1`, R0. Params: `digest`. Result: `SimulationReport`.
- **Library function:** `build_report(ReportInputs) -> Result<(SimulationReport, Digest), ReportError>` and `store_report`.
- **Data structures:** `SimulationReport { plan, machine_model, fixture, part_placement, probe_config, simulator: { id, version, fidelity }, verdicts: Vec<MoveVerdict>, unintended_contacts: Vec<Contact>, timing_model: { acceleration_m_s2 }, cycle_time_s, clean }`. The timing model is stored so `CMM.SIM.TIME` can rebuild the same timeline.
- **Errors:** `REPORT_INCOMPLETE`, `REPORT_CORRUPT`, `REPORT_NOT_FOUND`.
- **Determinism:** D0.
- **Crate:** `fid-cmm-sim`.

## Test fixtures

Synthetic report inputs in `fixtures/cmm/sim/` (`report_inputs_clean.json`, `report_inputs_margin.json`, `report_inputs_unchecked.json`), hand-written, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-sim`, implement the simulation report: type `SimulationReport` (plan, machine-model, fixture, part-placement and probe-config digests; simulator id, version and fidelity; one verdict per move; unintended contacts; cycle time; `clean`), pure `build_report`, content-addressed storage, and registry command `cmm.get_simulation_report@1` (R0; CLI `fid cmm get-simulation-report --json`; MCP generated).
> `clean` is true only if every verdict is CLEAR and there are no unintended contacts. Verdict count ≠ move count fails with `REPORT_INCOMPLETE`. Store under SHA-256 of RFC 8785 JCS; on read, re-hash and fail with `REPORT_CORRUPT` on mismatch.
> Acceptance tests: the six criteria in spec node `CMM.SIM.REPORT`, fixtures in `fixtures/cmm/sim/`.
> Constraints: no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0. The Motion Executive's simulation gate depends on this rule, so fail closed.

## Open questions

- None.

## Children

_Leaf: none._
