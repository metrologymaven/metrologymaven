---
id: QUAL.IQOQ
title: Automated IQ and OQ runs and qualification evidence bundle
level: 2
parent: QUAL
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-QUAL-001]
covers: []
depends_on: [TRUST.SIGN.TROOT, TRUST.SIGN.DSSE, UNC.RDL.AST, UNC.RDL.REP, CORE.RPL.EXEC.REPORT, CORE.JRN.ENTRY, CORE.REG.DEF, CORE.BUS.POL.FS, TRUST.PKG.MANIFEST, PLAT.DOCTOR.REPORT]
uses_artifacts: [KRN.MAN.MANI, ECO.REL.SIGN.SUMS, ECO.REL.SIGN.CUSTODY, QUAL.EVID.LAYOUT, UNC.RDL.FREEZE, TRUST.PKG.READER]
cli: [fid validate run-iq, fid validate run-oq, fid validate register-golden, fid validate show-record, fid validate export-bundle]
mcp: [validate_run_iq, validate_run_oq, validate_register_golden, validate_show_record, validate_export_bundle]
journal: [validate.run_iq@1, validate.run_oq@1, validate.register_golden@1, validate.export_bundle@1]
updated: 2026-11-17
---

# QUAL.IQOQ: Automated IQ and OQ runs and qualification evidence bundle

## Purpose

The executable half of the Validation Accelerator Pack. On the customer's own machine, offline, it runs an **installation qualification** (installed artefacts match what the project released and signed) and an **operational qualification** (the installed kernel reproduces reference results and the customer's own golden journals), writes one journaled qualification record per run, and exports the records as an evidence bundle that a customer person signs and `fid-verify` can check. It exists as its own subsystem because it is the only part of QUAL that runs inside the product as registry commands.

## Scope

**In:**
- `validate.run_iq@1`: check installed binaries, kernel artifact and data files against the release's signed checksum file and kernel manifest; check those signatures against the offline trust root; capture an environment snapshot
- `validate.run_oq@1`: run the point-of-use RDL subset through the RDL self-test runner, and replay every registered golden journal on the installed kernel, classified with REP-1
- `validate.register_golden@1`: register a recorded run as a golden journal for upgrade OQ, with its expected outcome digest
- The qualification record format (run kind, release version, digests checked, per-check verdict, environment snapshot digest, RDL subset digest, per-golden replay outcome)
- `validate.export_bundle@1`: package selected qualification records into an evidence bundle in an approved export root, ready for human signing
- Upgrade OQ: when a new version is installed, offer IQ and OQ and compare against the previous version's records

**Out** (and which node owns it instead):
- IQ/OQ/PQ protocol documents, URS and risk templates that describe these runs (owned by `QUAL.VAP`)
- The per-release evidence bundle that IQ checks against (owned by `QUAL.EVID`)
- The RDL runner and REP-1 classifier themselves (owned by `UNC.RDL.AST`, `UNC.RDL.REP`)
- Replay machinery (owned by `CORE.RPL.EXEC.REPORT`; golden journals are journals, not Result Packages, so `TRUST.REPLAY.RUN` is not used)
- Signing and key custody (owned by `TRUST.SIGN`; Regulated e-signature binding by `TRUST.ESIG`, after launch-1); this node never signs on its own
- Automated PQ: PQ is customer-specific and is guidance only (owned by `QUAL.VAP`)

## Requirements

- **REQ-QUAL.IQOQ-001** When `validate.run_iq@1` runs, the system shall compare the SHA-256 of every installed artefact listed in the release checksum file with the listed value and record each artefact as `MATCH`, `MISMATCH` or `MISSING`.
- **REQ-QUAL.IQOQ-002** When `validate.run_iq@1` runs, the system shall verify the checksum file signature and the kernel manifest signature against the locally installed offline trust root without any network access.
- **REQ-QUAL.IQOQ-003** If any artefact is `MISMATCH` or `MISSING`, or any signature check fails, then the IQ verdict shall be `FAIL` and the record shall list every failing item.
- **REQ-QUAL.IQOQ-004** When `validate.run_oq@1` runs, the system shall run the point-of-use RDL subset through `UNC.RDL.AST` and include the subset digest and per-case verdicts in the qualification record.
- **REQ-QUAL.IQOQ-005** When `validate.run_oq@1` runs and golden journals are registered, the system shall replay each one on the installed kernel and record its replay outcome (`IDENTICAL`, `EQUIVALENT`, `DIVERGENT` or `NOT_REPLAYABLE`) under REP-1.
- **REQ-QUAL.IQOQ-006** If any golden replay is `DIVERGENT`, then the OQ verdict shall be `FAIL` and the record shall list each characteristic whose value exceeded REP-1 or whose decision changed.
- **REQ-QUAL.IQOQ-007** Each IQ or OQ run shall commit exactly one journaled qualification record holding release version, kernel manifest digest, environment snapshot digest, per-check verdicts and the overall verdict.
- **REQ-QUAL.IQOQ-008** IQ and OQ shall run fully offline and without an account, and shall complete on a Tier 1 reference machine within 5 minutes for IQ and 15 minutes for OQ excluding golden replays [unverified].
- **REQ-QUAL.IQOQ-009** When `validate.export_bundle@1` runs, the system shall write the selected records and their referenced evidence into an approved export root as an unsigned evidence bundle that `fid-verify` can check for integrity.
- **REQ-QUAL.IQOQ-010** The evidence bundle shall become signed only through a human S1 act outside QUAL.IQOQ (a standard DSSE signature via `TRUST.SIGN` in the Lean profile; the `TRUST.ESIG` ceremony in the Regulated profile once available), and no IQ/OQ command shall perform an S1 act.
- **REQ-QUAL.IQOQ-011** When a new product version is first started and earlier qualification records exist, the system shall report that the installation is unqualified for the new version until IQ and OQ pass.

## Interfaces

- **Offers:** `validate.run_iq@1` (W2), `validate.run_oq@1` (W2), `validate.register_golden@1` (W2), `validate.show_record@1` (R0), `validate.export_bundle@1` (W1, `fs_effect: write_root`); the qualification record type; the IQ/OQ evidence bundle.
- **Consumes (code via `depends_on`, files via `uses_artifacts`):** release checksum file and SBOM references (`ECO.REL.SIGN`); kernel manifest (`KRN.MAN.MANI`); trust root check (`TRUST.SIGN.TROOT`); RDL runner (`UNC.RDL.AST`) and REP-1 (`UNC.RDL.REP`); golden replay (`CORE.RPL.EXEC.REPORT`); package container (`TRUST.PKG.MANIFEST`); environment report (`PLAT.DOCTOR.REPORT`); per-release evidence digests (`QUAL.EVID`).

## Decomposition hints

IQ (artefact and signature checks), OQ (RDL subset plus golden replay), GOLDEN (golden-journal registry), RECORD (record format and show), BUNDLE (export for signing), UPGRADE (version-change detection and prompt).

## Split rationale

The split follows the commands, plus the shared record they all write or read: RECORD (record type, show command and qualification status, which also covers upgrade detection), IQ, OQ, GOLDEN (registration, human-only) and BUNDLE (export). UPGRADE from the hints is folded into RECORD because "unqualified for the new version" is a status computed from records, not a separate run.

Alternatives considered:
- **A separate UPGRADE child.** Rejected: it would be a few lines over RECORD's query.
- **Golden registration inside OQ.** Rejected: it is a human-only command with a different principal rule and its own errors.

## Open questions

- Amended for launch-1: REQ-QUAL.IQOQ-010 no longer requires `TRUST.ESIG` (excluded from launch-1); the Lean profile uses a human DSSE signature through `TRUST.SIGN`, and the dependency on `TRUST.ESIG.BIND` is removed. Launch-1 still lacks a human-facing package-signing command in TRUST (cross-domain).
- Layer: CONSTITUTION §2.1 does not place QUAL product crates. `fid-qual` orchestrates TRUST, UNC, CORE.RPL and PLAT, so L3 is proposed; the steering group should confirm.
- Whether the TRUST Result Package container admits qualification records as a payload kind is not yet specified by `TRUST.PKG`.
- Customer golden journals may contain export-controlled geometry; the bundle carries only digests and outcomes (`QUAL.IQOQ.BUNDLE`).
- Which signature meaning a customer uses for IQ/OQ approval matters only in the Regulated profile (`TRUST.ESIG.MEANING`, after launch-1).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| QUAL.IQOQ.RECORD | Qualification record type, show command and qualification status | 007 011 | none |
| QUAL.IQOQ.IQ | Installation qualification run | 001 002 003 008 | none |
| QUAL.IQOQ.OQ | Operational qualification run | 004 005 006 008 | none |
| QUAL.IQOQ.GOLDEN | Golden-journal registration for upgrade OQ | 005 | none |
| QUAL.IQOQ.BUNDLE | Qualification evidence bundle export | 009 010 | none |
