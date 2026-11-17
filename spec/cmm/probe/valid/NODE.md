---
id: CMM.PROBE.VALID
title: Qualification validity and probe status query
level: 3
parent: CMM.PROBE
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-CMM.PROBE-008, REQ-CMM.PROBE-009]
covers: []
depends_on: [CMM.PROBE.MODEL]
side_effect: R0
determinism: D0
cli: [fid cmm get-probe-status]
mcp: [cmm_get_probe_status]
journal: [cmm.get_probe_status@1]
updated: 2026-11-16
---

# CMM.PROBE.VALID: Qualification validity and probe status query

## Purpose

Decides whether each tip of a probe configuration is currently qualified. It owns the validity policy type (expiry duration and temperature drift window), the pure function that turns a passing qualification into a validity period, and the read-only status query that the run-ticket validator, path planner and UI call: "for this configuration, at this time and temperature, which tips are valid, and what is the earliest `valid_until`?".

## Scope

**In:**
- `ValidityPolicy { expiry_s, drift_window_k }` and `ValidityPeriod { qualified_at, valid_until, temperature_at_qualification_k, drift_window_k }`
- `validity_for(record_inputs, policy) -> ValidityPeriod`, a pure function called by `CMM.PROBE.QUAL` when it issues a record
- `cmm.get_probe_status@1` (R0): per-tip status at an explicit time and optional current temperature
- Status reasons: `VALID`, `EXPIRED`, `DRIFT_EXCEEDED`, `TEMPERATURE_UNKNOWN`, `NEVER_QUALIFIED`, `UNQUALIFIED`, `CONFIG_CHANGED`

**Out** (and which node owns it instead):
- Producing qualification records (owned by `CMM.PROBE.QUAL`)
- Reading machine temperatures (owned by `CMM.EXEC.RECORD` and `HAL`; the caller passes the temperature)
- Enforcing validity at arming (owned by `CMM.EXEC.TICKET` and `CMM.EXEC.VALID`)

## Requirements

- **REQ-CMM.PROBE.VALID-001** The system shall compute a validity period from a passing qualification as `valid_until = qualified_at + expiry_s` and a temperature window of `temperature_at_qualification ± drift_window_k`, with both policy values finite and positive.
- **REQ-CMM.PROBE.VALID-002** When status is queried for a probe configuration at time t with an optional current temperature T, the system shall report each tip `VALID` only if its latest record for the current configuration digest passed, t < `valid_until`, and T lies inside its temperature window.
- **REQ-CMM.PROBE.VALID-003** If the query time is at or after `valid_until`, then the system shall report the tip `EXPIRED`; if T lies outside the window, `DRIFT_EXCEEDED`; if the policy has a drift window and no T is supplied, `TEMPERATURE_UNKNOWN`.
- **REQ-CMM.PROBE.VALID-004** If a tip's latest qualification record refers to a different configuration digest than the current version, then the system shall report it `CONFIG_CHANGED` and not `VALID`.
- **REQ-CMM.PROBE.VALID-005** The status result shall contain the configuration digest, per-tip status and reason, and the earliest `valid_until` over the tips requested (or over all tips when none are named).
- **REQ-CMM.PROBE.VALID-006** The status query shall take the evaluation time as an explicit parameter (the CLI fills the current time and records it), so the same inputs always give the same result.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/cmm/probe/status_records.json` (star5 configuration, five passing records qualified at 2026-01-01T08:00:00Z at 293.15 K, policy expiry 8 h, drift 1 K), when `fid cmm get-probe-status --probe <id> --at 2026-01-01T12:00:00Z --temperature 293.6K --json` runs, then all five tips are `VALID` and `earliest_valid_until` is 2026-01-01T16:00:00Z.
- **AC-2** Given the same fixture, when queried at 2026-01-01T16:00:00Z, then all tips are `EXPIRED`.
- **AC-3** Given the same fixture, when queried at 12:00 with temperature 294.5 K, then all tips are `DRIFT_EXCEEDED`.
- **AC-4** Given the same fixture, when queried without a temperature, then all tips are `TEMPERATURE_UNKNOWN`.
- **AC-5** Given fixture `fixtures/cmm/probe/status_config_changed.json` (records against version 1, configuration now at version 2), when queried, then all tips are `CONFIG_CHANGED`.
- **AC-6** Given fixture `status_records.json` with tip 4 recorded `UNQUALIFIED`, when queried at 12:00 at 293.6 K, then tip 4 is `UNQUALIFIED`, the other four are `VALID`.
- **AC-7** (replay) Given the journal entry from AC-1, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command:** `cmm.get_probe_status@1`, R0. Params: `probe_id`, `version: Option<u32>`, `tips: Vec<TipId>` (empty = all), `at: Timestamp`, `temperature: Option<Quantity<K>>`. Result: `ProbeStatus { config_digest, tips: Vec<TipStatus { tip_id, status, reason_detail, valid_until }>, earliest_valid_until: Option<Timestamp> }`.
- **Library function:** `validity_for(qualified_at, temperature_k, policy) -> Result<ValidityPeriod, ValidityError>`.
- **Algorithm:** load latest record per tip (records ordered by journal position); compare digests; apply checks in the order NEVER_QUALIFIED, UNQUALIFIED, CONFIG_CHANGED, EXPIRED, TEMPERATURE_UNKNOWN, DRIFT_EXCEEDED, VALID; earliest by total-order minimum.
- **Errors:** `PROBE_NOT_FOUND`, `TIP_NOT_FOUND`, `VALIDITY_POLICY_INVALID`.
- **Determinism:** D0. Time and temperature are inputs; integer-nanosecond timestamps; no wall-clock reads in the command body.
- **Crate:** `fid-cmm-probe`.

## Test fixtures

Synthetic record sets in `fixtures/cmm/probe/` (`status_records.json`, `status_config_changed.json`) built on `star5_config.json`, hand-written, CC-BY-4.0.

## Build prompt

> In the Rust crate `fid-cmm-probe`, implement qualification validity: the types `ValidityPolicy { expiry_s, drift_window_k }` and `ValidityPeriod`, the pure function `validity_for(qualified_at, temperature_k, policy)`, and the registry command `cmm.get_probe_status@1` (R0; CLI `fid cmm get-probe-status --probe <id> [--at <ts>] [--temperature <q>] --json`; MCP `cmm_get_probe_status`, generated).
> Per tip, using the latest qualification record: `NEVER_QUALIFIED`, `UNQUALIFIED`, `CONFIG_CHANGED` (record digest ≠ current config digest), `EXPIRED` (at ≥ valid_until), `TEMPERATURE_UNKNOWN` (policy has drift window, no temperature given), `DRIFT_EXCEEDED` (outside ± window), else `VALID`. Result has config digest, per-tip status and earliest valid_until. Evaluation time is an explicit parameter; the CLI fills "now" and records it.
> Acceptance tests: the seven criteria in spec node `CMM.PROBE.VALID`, fixtures in `fixtures/cmm/probe/`.
> Constraints: no wall-clock reads inside the command; no panic, no `unsafe`, no `unwrap`/`expect`; total-order min; determinism D0.

## Open questions

- Default expiry and drift window need a domain expert; no default is set, so a policy must be supplied at qualification.

## Children

_Leaf: none._
