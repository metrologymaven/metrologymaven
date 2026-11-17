---
id: CMM.SIM.GATE
title: Clean-simulation query for a digest tuple
level: 3
parent: CMM.SIM
kind: feature
status: leaf
phase: P2
size: S
owners: []
claimed_by:
satisfies: [REQ-CMM.SIM-006, REQ-CMM.SIM-008]
covers: []
depends_on: [CMM.SIM.REPORT]
side_effect: R0
determinism: D0
cli: [fid cmm find-clean-simulation]
mcp: [cmm_find_clean_simulation]
journal: [cmm.find_clean_simulation@1]
updated: 2026-11-16
---

# CMM.SIM.GATE: Clean-simulation query for a digest tuple

## Purpose

Answers the Motion Executive's simulation-gate question: for this exact (plan, machine, setup, probe configuration) digest tuple, is there a stored clean simulation report, and which one? It is a read-only index over stored reports, available to agents (R0) and to the Motion Executive.

## Scope

**In:**
- Tuple key: plan digest, machine-model digest, setup digest (SHA-256 of the JCS pair of fixture and part-placement digests), probe-configuration digest
- Index from tuple key to report digests, maintained when reports are stored
- `cmm.find_clean_simulation@1` (R0)

**Out** (and which node owns it instead):
- Report content and the clean rule (owned by `CMM.SIM.REPORT`)
- Deciding which fidelity levels are acceptable for a ticket (owned by `CMM.EXEC.VALID`)

## Requirements

- **REQ-CMM.SIM.GATE-001** When queried with a (plan, machine, setup, probe configuration) digest tuple, the system shall return the digest of a stored report that is clean and matches all four digests exactly, or the typed result `NO_CLEAN_SIMULATION`.
- **REQ-CMM.SIM.GATE-002** If several clean reports match, then the system shall return the one stored last, and shall list the simulator fidelity level of each match.
- **REQ-CMM.SIM.GATE-003** The system shall re-verify the returned report's digest before answering and shall answer `NO_CLEAN_SIMULATION` with reason `REPORT_CORRUPT` if verification fails.
- **REQ-CMM.SIM.GATE-004** The query shall be class R0 and available over MCP.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/sim/gate_store.json` (three stored reports: clean for tuple T1, not clean for T1, clean for T2), when `fid cmm find-clean-simulation --plan <p1> --machine <m> --setup <s> --probe <c> --json` runs for T1, then the clean T1 report digest is returned.
- **AC-2** Given the same store, when queried for T1 with a probe digest differing in one byte, then the result is `NO_CLEAN_SIMULATION`.
- **AC-3** Given a store with two clean T1 reports, when queried, then the later one is returned and both fidelity levels are listed.
- **AC-4** (error path) Given a corrupted stored blob for the only clean T1 report, when queried, then the result is `NO_CLEAN_SIMULATION` with reason `REPORT_CORRUPT`.
- **AC-5** (replay) Given the journal from AC-1, when replayed in exact mode against the same store, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.find_clean_simulation@1`, R0. Params: `plan`, `machine_model`, `setup`, `probe_config` (digests). Result: `Found { report, fidelity, matches: Vec<{report, fidelity}> } | NoCleanSimulation { reason }`.
- **Data structures:** ordered index (BTreeMap) keyed by the four-digest tuple, values in store order.
- **Errors:** `DIGEST_MALFORMED`; corruption is a typed result, not an error.
- **Determinism:** D0.
- **Crate:** `fid-cmm-sim`.

## Test fixtures

`fixtures/cmm/sim/gate_store.json` built from the `CMM.SIM.REPORT` fixtures, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-sim`, implement registry command `cmm.find_clean_simulation@1` (R0; CLI `fid cmm find-clean-simulation --json`; MCP `cmm_find_clean_simulation`, generated) over an ordered index of stored simulation reports keyed by (plan, machine model, setup, probe config) digests, where setup = SHA-256 of JCS {fixture, part_placement}.
> Return the latest clean report matching all four digests exactly, with each match's fidelity level, or `NO_CLEAN_SIMULATION`. Re-verify the returned report's digest; on failure answer `NO_CLEAN_SIMULATION` with reason `REPORT_CORRUPT`.
> Acceptance tests: the five criteria in spec node `CMM.SIM.GATE`, fixture `fixtures/cmm/sim/gate_store.json`.
> Constraints: no std HashMap; no panic, no `unsafe`, no `unwrap`/`expect`; determinism D0; fail closed.

## Open questions

- Whether a clean report at fidelity L2 should satisfy a gate that asks for L1 is left to `CMM.EXEC.VALID`; this query reports levels and does not filter.

## Children

_Leaf: none._
