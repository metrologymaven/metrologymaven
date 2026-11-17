---
id: UNC.INST.LIB.REG
title: Instrument and gauge register
level: 4
parent: UNC.INST.LIB
kind: feature
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.INST.LIB-005, REQ-UNC.INST.LIB-006]
covers: [SA.UNC.01]
depends_on: [UNC.INST.LIB.FORMAT, UNC.INST.SCHEMA, HAL.IDL.PROFILE, CORE.STO.CAS]
side_effect: [W2, W2, W2, R0, R0]
determinism: D0
cli: [fid unc add-instrument, fid unc update-instrument, fid unc retire-instrument, fid unc list-instrument, fid unc show-instrument]
mcp: [unc_add_instrument, unc_update_instrument, unc_retire_instrument, unc_list_instrument, unc_show_instrument]
journal: [unc.add_instrument@1, unc.update_instrument@1, unc.retire_instrument@1, unc.list_instrument@1, unc.show_instrument@1]
updated: 2026-11-17
---

# UNC.INST.LIB.REG: Instrument and gauge register

## Purpose

The single register of the site's measuring instruments and hand gauges (agreed owner at review): CMMs, arms, trackers, scanners, digital and analogue hand gauges, indicators and environment sensors. Each entry holds identity, kind, resolution, measuring range, an optional profile (which carries the MPE terms and the uncertainty model), calibration due date and certificate reference. HAL cites an entry's profile digest in acquisition stamps, RPT checks manual readings against it, and the first-run wizard's step 5 is satisfied by its write commands.

## Scope

**In:**
- `RegisterEntry` record and its storage in the project's settings scope (site-wide) with history
- Commands `unc.add_instrument@1`, `unc.update_instrument@1` (including recording a new calibration), `unc.retire_instrument@1` (W2), `unc.list_instrument@1`, `unc.show_instrument@1` (R0)
- Query functions: `calibration_overdue(entry, date)`, `resolution_decimals(entry, unit)`, `profile_ref(entry) -> Digest | none`, `entry_at(entry, date)` (the entry state valid on a date)

**Out:**
- Profile library content and format (owned by `UNC.INST.LIB.FORMAT` and `UNC.INST.LIB.CURATE`)
- Binding a live device instance to a model (owned by `UNC.INST.SCHEMA`)
- Driver-connected gauge input and runtime binding (owned by `HAL.DRV.GAU`); file-source binding (owned by `HAL.ACQ.FILEDEV`)
- Manual reading entry and its UEC rules (owned by `RPT.BALLOON.ENTRY`)

## Requirements

- **REQ-UNC.INST.LIB.REG-001** A register entry shall hold a unique entry ID, kind (`cmm | arm | tracker | scanner | hand_gauge | indicator | env_sensor | other`), make, model and serial as untrusted text, resolution as an SI quantity, measuring range as SI min and max, an optional profile digest, a calibration due date, an optional certificate document digest, and status `active | retired`.
- **REQ-UNC.INST.LIB.REG-002** If `unc.add_instrument@1` or `unc.update_instrument@1` is given a duplicate entry ID, a non-positive resolution, an inverted range, or a profile digest that does not resolve in the profile library or among imported profiles, then the command shall fail with `entry_exists`, `invalid_resolution`, `invalid_range` or `profile_not_found` and commit nothing.
- **REQ-UNC.INST.LIB.REG-003** Every add, update and retire shall journal one event with the entry digest, and `unc.retire_instrument@1` shall keep the entry and its history so past readings and stamps still resolve.
- **REQ-UNC.INST.LIB.REG-004** When `calibration_overdue(entry, date)` is called, the system shall use the entry state valid on that date and return `overdue` if the date is after its calibration due date, `ok` otherwise, and `unknown` if no due date was recorded.
- **REQ-UNC.INST.LIB.REG-005** When `resolution_decimals(entry, unit)` is called, the system shall return the number of decimal places the entry's resolution supports in that unit (for example 2 for 0.01 mm in mm).
- **REQ-UNC.INST.LIB.REG-006** When `profile_ref(entry)` is called, the system shall return the profile digest recorded for the entry state in force, or the explicit value `none`.
- **REQ-UNC.INST.LIB.REG-007** When `unc.list_instrument@1` is called, the system shall return entries sorted by ID, filterable by kind and status, each with its calibration status on the supplied date.

## Acceptance criteria

- **AC-1** Given an empty project, when `fid unc add-instrument --id CAL-001 --kind hand_gauge --resolution 0.01mm --range 0mm:150mm --cal-due 2027-03-01 --json` runs, then the entry is stored with resolution 1e-5 m and one `unc.add_instrument@1` event is journaled.
- **AC-2** Given AC-1 and the profile library from `fixtures/unc/inst/lib/release_1_0_0/`, when `fid unc add-instrument --id ARM-01 --kind arm --profile <arm profile digest>` runs, then `profile_ref(ARM-01)` returns that digest; for `CAL-001` it returns `none`.
- **AC-3** (calibration) Given AC-1, then `calibration_overdue(CAL-001, 2027-03-02)` is `overdue` and `(CAL-001, 2027-03-01)` is `ok`; after `update-instrument --cal-due 2028-03-01` committed on 2027-03-05, the query for 2027-03-02 still returns `overdue` and for 2027-03-06 returns `ok`.
- **AC-4** Given AC-1, then `resolution_decimals(CAL-001, mm)` is 2 and `(CAL-001, in)` is 4 [unverified: rounding rule for non-decimal unit conversions].
- **AC-5** (error paths) Given `fixtures/unc/inst/lib/reg_bad.json` (duplicate ID, resolution 0, range 10 mm to 5 mm, unknown profile digest), each fails with its code and the journal head is unchanged.
- **AC-6** (retire) Given `CAL-001` retired, then `list-instrument --status active` omits it, `show-instrument --id CAL-001` still returns it with history.
- **AC-7** (replay) Given the journal from AC-1 to AC-6, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):** `unc.add_instrument@1` (W2; fields of REQ-001, `certificate: Option<ImportHandle>`); `unc.update_instrument@1` (W2; `id` plus changed fields, `effective: Date`); `unc.retire_instrument@1` (W2; `id`, `effective: Date`, `reason: UntrustedText`); `unc.list_instrument@1` (R0; `kind`, `status`, `on_date`); `unc.show_instrument@1` (R0; `id`).
- **Data structures:** `RegisterEntry` (REQ-001) with `history: Vec<(effective Date, EntryState digest)>`; entry digest = SHA-256 over RFC 8785 canonical JSON. Stored site-wide so every project of the site sees one register.
- **Algorithm:** state on date = last history item with `effective` ≤ date; decimals = smallest d ≥ 0 such that resolution in the unit is an integer multiple of 10^-d within 1e-9 relative, capped at 9.
- **Errors:** `entry_exists`, `entry_not_found`, `invalid_resolution`, `invalid_range`, `profile_not_found`, `text_too_long`.
- **Determinism:** D0 (dates are inputs, never the wall clock).
- **Performance budget:** list 10 000 entries in under 50 ms [unverified until measured].
- **Crate:** `fid-unc-inst` (module `register`).

## Test fixtures

`fixtures/unc/inst/lib/`: the library release from `UNC.INST.LIB.FORMAT`, `reg_bad.json`, invented gauges and instruments (CC-BY-4.0).

## Build prompt

> In crate `fid-unc-inst`, module `register`, implement the single instrument and gauge register. Entry: unique ID, kind (`cmm|arm|tracker|scanner|hand_gauge|indicator|env_sensor|other`), make/model/serial as untrusted text (≤ 4 096 bytes, never interpreted), resolution (SI, > 0), range (SI min ≤ max), optional profile digest (must resolve in the `fid-unc-inst` profile library or among imported `instrument-profile/v1` descriptors), calibration due date, optional certificate blob digest (content-addressed store), status `active|retired`, dated history.
> Registry commands: `unc.add_instrument@1`, `unc.update_instrument@1`, `unc.retire_instrument@1` (W2, journaled with the entry digest; retire keeps the entry), `unc.list_instrument@1`, `unc.show_instrument@1` (R0). CLI `fid unc <verb>-instrument`, MCP `unc_<verb>_instrument`, `--json`, `--dry-run` for writes. Errors `entry_exists`, `entry_not_found`, `invalid_resolution`, `invalid_range`, `profile_not_found`, `text_too_long`; failures commit nothing.
> Public query functions for HAL and RPT: `calibration_overdue(id, date) -> overdue|ok|unknown` using the entry state in force on that date; `resolution_decimals(id, unit)`; `profile_ref(id) -> Digest | none`; `entry_at(id, date)`. Dates are inputs, never the wall clock. Tests AC-1..AC-7. No panic, `unwrap`, `expect`; BTreeMap only (CONSTITUTION §8).

## Open questions

- `FLOW.SETUP.STEPS` step 5 should bind to `unc.add_instrument@1` and `unc.update_instrument@1` (cross-node note for FLOW).
- `RPT.BALLOON.GAUGE` should become a view over this register and drop its own `report.*_gauge` write commands (cross-domain note for RPT).
- Whether register writes by agents are allowed at all (they would land on an agent branch as W2) or should be `human_only` like manual readings is for UNC and TRUST owners; this leaf leaves them ordinary W2.

## Children

_Leaf: none._
