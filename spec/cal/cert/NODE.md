---
id: CAL.CERT
title: ISO/IEC 17025 certificates, CMC guardbanding and PTB DCC XML
level: 2
parent: CAL
kind: subsystem
status: open
phase: P4
size: L
owners: []
claimed_by:
satisfies: [REQ-CAL-004]
covers: []
depends_on: [UNC.DEC, RPT.OUTPUT, TRUST.SIGN, IO.REG]
cli: [fid cal emit-cert, fid cal export-dcc, fid cal import-dcc]
mcp: [cal_emit_cert, cal_export_dcc, cal_import_dcc]
journal: [cal.emit_cert, cal.export_dcc, cal.import_dcc]
updated: 2026-11-22
---

# CAL.CERT: ISO/IEC 17025 certificates, CMC guardbanding and PTB DCC XML

## Purpose

Generates human-readable and machine-readable calibration certificates aligned with ISO/IEC 17025 data fields, explicit CMC floor checks, ILAC-G8 / JCGM 106 guardbanding statements, DSSE cryptographic signatures (`TRUST`), and bounded PTB Digital Calibration Certificate (DCC) XML import and export.

## Scope

**In:** ISO/IEC 17025 required data-field schema checks (never reproducing standards text), CMC floor enforcement (reported expanded uncertainty never below accredited CMC), TUR and ILAC-G8 decision-rule statements, bounded PTB DCC XML reader/writer, and signed PDF + DCC archive packaging.
**Out:** General part inspection reports (`RPT.FORMS`); calibration math procedures (`CAL.CMM`, `CAL.MACH`, `CAL.GAUGE`).

## Requirements

- **REQ-CAL.CERT-001** The subsystem shall refuse to emit a calibration certificate when expanded uncertainty is below the declared CMC floor or when required traceability/environment fields are absent.
- **REQ-CAL.CERT-002** The subsystem shall import and export bounded PTB DCC XML documents without panicking on hostile input and sign emitted certificate packages via `TRUST.SIGN`.

## Children

_None yet: status `open`._
