---
id: UNC.INST.IEMPS
title: Instrument Error Model Parameter Sets for virtual-CMM simulation
level: 3
parent: UNC.INST
kind: feature
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.INST-007]
covers: [SA.UNC.01]
depends_on: [UNC.INST.SCHEMA, CORE.STO.CAS, CORE.BUS.PRIN]
side_effect: [W2, W2, R0]
determinism: D0
cli: [fid unc import-acceptance-record, fid unc import-iemps, fid unc show-iemps]
mcp: [unc_import_iemps, unc_show_iemps]
journal: [unc.import_acceptance_record@1, unc.import_iemps@1]
updated: 2026-11-17
---

# UNC.INST.IEMPS: Instrument Error Model Parameter Sets for virtual-CMM simulation

## Purpose

Ingests Instrument Error Model Parameter Sets (IEMPS): maker- or user-supplied parameter sets (geometric error maps, probing parameters, thermal parameters) that drive an ISO/TS 15530-4 virtual-CMM simulation. An IEMPS becomes bindable for simulation only when it is anchored to a passing ISO 10360-2 acceptance or reverification record for the same machine, still inside the IEMPS's declared validity period. Because no other node stores those externally performed test records, this leaf also owns recording them as human-entered data.

## Scope

**In:**
- IEMPS data format: an `instrument-model/v1` model of kind `iemps` with an `iemps` extension block (error-map grids, probe parameters, thermal terms, validity period, anchor reference)
- The acceptance record: machine identity, test kind (`acceptance | reverification`), test date, verdict, issuer, certificate document digest
- `unc.import_acceptance_record@1` (W2, `human_only: true`): transcription of an externally performed test
- `unc.import_iemps@1` (W2) with the anchoring check and bounded grids
- `unc.show_iemps@1` (R0)
- `iemps_bindable(digest, on_date) -> Bindability`, the predicate every simulation binding calls

**Out:**
- The base model format, `unc.bind_instrument_model@1` and the range check (owned by `UNC.INST.SCHEMA`)
- Running the virtual-CMM simulation (owned by `UNC.PROP.MCM`)
- Verifying signatures on maker documents (owned by `TRUST`; this leaf stores the signature block opaquely)
- Performing or evaluating ISO 10360 tests (not offered; see `HAL.LIVE.CHECK` scope)

## Requirements

- **REQ-UNC.INST.IEMPS-001** When an IEMPS is imported, the system shall resolve its anchor reference to a stored acceptance record and check that the record's machine identity equals the IEMPS machine identity and its verdict is `pass`.
- **REQ-UNC.INST.IEMPS-002** If the anchor record is missing, has verdict `fail`, names a different machine, or has a test date earlier than the evaluation date minus the IEMPS validity period, then the system shall store the IEMPS with state `not_bindable` and the reason code (`anchor_missing`, `anchor_failed`, `anchor_machine_mismatch`, `anchor_expired`).
- **REQ-UNC.INST.IEMPS-003** When `iemps_bindable` is called for an IEMPS digest and a date, the system shall re-evaluate the anchoring rules for that date and return `bindable` or `not_bindable` with the reason, so that a binding for simulation fails on any IEMPS that is not bindable on that date.
- **REQ-UNC.INST.IEMPS-004** Every IEMPS shall carry its source kind (`maker | user`), semver and SHA-256 digest of its RFC 8785 canonical form, and the import result shall return that digest for results to reference.
- **REQ-UNC.INST.IEMPS-005** If an imported IEMPS has more than 10^6 error-map grid nodes in total [unverified: default bound] or any grid whose declared node count disagrees with its value array, then import shall fail with `grid_too_large` or `grid_inconsistent` and commit nothing.
- **REQ-UNC.INST.IEMPS-006** If `unc.import_acceptance_record@1` is called by an agent or script principal, then it shall fail with `principal_not_human` and commit nothing.
- **REQ-UNC.INST.IEMPS-007** The acceptance record's issuer and every free-text field of an IEMPS shall be stored as untrusted text of at most 4 096 bytes and never interpreted (CONSTITUTION C6).

## Acceptance criteria

- **AC-1** Given `fixtures/unc/inst/iemps/acc_pass.json` (machine `sim-cmm-1`, verdict pass, date 2026-06-01) imported by a human, and `fixtures/unc/inst/iemps/sim_cmm_iemps.json` (validity 365 days, anchored to that record), when `fid unc import-iemps --file <handle> --on-date 2026-11-17 --json` runs, then the IEMPS is stored with state `bindable`, its digest equals `fixtures/unc/inst/iemps/digests.txt`, and one `unc.import_iemps@1` event is journaled.
- **AC-2** (expired) Given AC-1's IEMPS, when `iemps_bindable(digest, 2027-07-01)` is called, then it returns `not_bindable` with `anchor_expired`.
- **AC-3** (error paths) Given `iemps_fail_anchor.json`, `iemps_other_machine.json` and `iemps_no_anchor.json`, when imported, then each is stored `not_bindable` with `anchor_failed`, `anchor_machine_mismatch` and `anchor_missing` respectively.
- **AC-4** (bounds) Given `iemps_huge_grid.json` (1 000 001 nodes) and `iemps_grid_mismatch.json`, when imported, then they fail with `grid_too_large` and `grid_inconsistent` and the journal head is unchanged.
- **AC-5** (principal) Given a script principal calling `unc.import_acceptance_record@1`, then it fails with `principal_not_human`; and the MCP tool list generated from the registry offers it only as a request-only projection.
- **AC-6** Given AC-1, when `fid unc show-iemps --id <id> --json` runs, then it returns source kind, semver, digest, machine identity, anchor digest, state and reason.
- **AC-7** (replay) Given the journal from AC-1 and AC-3, when replayed in exact mode, then the outcome is `IDENTICAL`.

## Design

- **Registry command(s):**
  - `unc.import_acceptance_record@1` (W2, `human_only: true`; params `machine: DeviceId`, `kind: acceptance|reverification`, `test_date: Date`, `verdict: pass|fail`, `issuer: UntrustedText`, `document: ImportHandle`; result `AcceptanceRecordRef { digest }`).
  - `unc.import_iemps@1` (W2; params `file: ImportHandle`, `on_date: Date`; result `IempsImport { id, digest, state, reason }`). Dry-run supported.
  - `unc.show_iemps@1` (R0; `id`).
- **Data structures:** `AcceptanceRecord { machine, kind, test_date, verdict, issuer: UntrustedText, document_digest }`; `IempsBlock { machine, source: maker|user, validity_days: u32, anchor: Digest, error_maps: Vec<ErrorMap{ name, axis, start, step, count, unit, values }>, probe: Vec<Param>, thermal: Vec<Param>, signature: Option<OpaqueBlock> }`; `Bindability = Bindable | NotBindable{reason}`. Error-map names are project-defined identifiers (axis and component), never standards text.
- **Algorithm:** bounded parse (≤ 64 MB, depth ≤ 32) through `UNC.INST.SCHEMA` validation of the base model; check the IEMPS block; count grid nodes before allocating; store the document blob in the content-addressed store; evaluate the anchor rules with `on_date` as an explicit, journaled input (never the wall clock).
- **Errors:** `grid_too_large`, `grid_inconsistent`, `principal_not_human`, `schema_invalid{field}` (from SCHEMA), `version_conflict`.
- **Determinism:** D0; the evaluation date is an input.
- **Performance budget:** import of a 10^6-node IEMPS in under 2 s [unverified until measured].
- **Crate:** `fid-unc-inst` (module `iemps`).

## Test fixtures

`fixtures/unc/inst/iemps/`: synthetic acceptance records for simulated machine `sim-cmm-1` (from `HAL.SIM`), project-authored IEMPS files with invented error maps, invalid variants and digests (CC-BY-4.0). No maker data and no standards text.

## Build prompt

> In crate `fid-unc-inst`, module `iemps`, add the IEMPS extension of `instrument-model/v1` (kind `iemps`): machine identity, source `maker|user`, `validity_days`, anchor digest, error-map grids `{name, axis, start, step, count, unit, values}`, probe and thermal parameters, optional opaque signature block (not verified here). Reuse the `fid-unc-inst` schema validation and canonical SHA-256 digest (RFC 8785).
> Implement registry commands: `unc.import_acceptance_record@1` (W2, `human_only: true`, CLI `fid unc import-acceptance-record`, no MCP tool beyond the generated request-only projection; refuses agent and script principals with `principal_not_human`; stores the certificate document via the content-addressed store), `unc.import_iemps@1` (W2, CLI `fid unc import-iemps`, MCP `unc_import_iemps`; import-root handle only; `on_date` input) and `unc.show_iemps@1` (R0). Provide `iemps_bindable(digest, date) -> Bindable | NotBindable{reason}` with reasons `anchor_missing`, `anchor_failed`, `anchor_machine_mismatch`, `anchor_expired` (test date earlier than date minus validity). Not-bindable IEMPS are stored, never rejected. Fail closed with `grid_too_large` (> 10^6 total nodes, counted before allocation) or `grid_inconsistent`, committing nothing. Untrusted text ≤ 4 096 bytes, never interpreted (C6).
> Tests: AC-1..AC-7 with `fixtures/unc/inst/iemps/`. `--json` and `--dry-run`. No panic, `unwrap`, `expect` or `unsafe`; BTreeMap only (CONSTITUTION §8).

## Open questions

- Licensing or interfacing PTB's VCMM for ISO/TS 15530-4 is open [unverified].
- No maker publishes an IEMPS format today as far as we know [unverified]; the format may need to be proposed to makers.
- `UNC.INST.SCHEMA`'s `unc.bind_instrument_model@1` and `UNC.PROP.MCM`'s virtual-CMM binding should call `iemps_bindable`; neither node names it yet (cross-node note for their owners).
- The acceptance record is a human transcription of an external certificate. Whether TRUST should later verify a signed digital calibration certificate in its place is for TRUST's owners.
- `fid unc import-iemps` and `fid unc import-acceptance-record` fall outside the `UNC.INST` CLI patterns (`*-instrument-model`, `*-profile`); the parent's patterns should be widened by its owner.

## Children

_Leaf: none._
