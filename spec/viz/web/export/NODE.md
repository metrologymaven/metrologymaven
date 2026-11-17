---
id: VIZ.WEB.EXPORT
title: ViewerPackage export command
level: 3
parent: VIZ.WEB
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-VIZ.WEB-008]
covers: []
depends_on: [VIZ.WEB.PKG, TRUST.SIGN, CORE]
cli: [fid view export-package]
mcp: [view_export_package]
journal: [view.export_package@*]
updated: 2026-11-16
---

# VIZ.WEB.EXPORT: ViewerPackage export command

## Purpose

Assemble a ViewerPackage from the working document: select content, apply redactions, compute display data and f64 sidecars from kernel results, include NamedViews, request signing from TRUST, and write the package to an approved export root.

## Scope

**In:**
- `view.export_package@1` (W1 with `fs_effect: write_root` per D-007; X1 only for off-machine destinations)
- Content selection and redaction rules
- Display-data and sidecar generation from kernel results (no new computation)
- Signing request to TRUST

**Out** (and which node owns it instead):
- Format (`VIZ.WEB.PKG`); signing (`TRUST.SIGN`)
- Network egress (a general X1 egress command; not owned by VIZ)

## Requirements

- **REQ-VIZ.WEB.EXPORT-001** `view.export_package@1` shall build the package only from kernel results and display data already in the document, computing no new value of record.
- **REQ-VIZ.WEB.EXPORT-002** Every redaction rule applied shall be recorded in the package manifest and in the journal entry.
- **REQ-VIZ.WEB.EXPORT-003** The command shall write only to an approved export root identified by a handle, and shall be journaled with the package digest.
- **REQ-VIZ.WEB.EXPORT-004** If signing is requested and fails, then no package file shall be written.

## Interfaces

- **Offers:** `view.export_package@1`.
- **Consumes:** `VIZ.WEB.PKG`; `TRUST.SIGN`; CORE storage roots and journal.

## Decomposition hints

SELECT, REDACT, BUILD (parts and sidecars), SIGN, WRITE.

## Open questions

- Agent-authored exports: whether an agent may call this W1 command directly or only propose it is governed by CORE.BUS rules for file writes.

## Children

_None yet: status `open`._
