---
id: IO.CAD.XLATE
title: Native CAD translator plugin contract
level: 3
parent: IO.CAD
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.CAD-011]
covers: [GOM.IMP.06, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.REG.PLUGIN, IO.CAD.STEP, IO.CAD.PMI]
cli: [fid import cad-native]
mcp: [import_cad_native]
journal: [import.cad_native@1]
updated: 2026-11-17
---

# IO.CAD.XLATE: Native CAD translator plugin contract

## Purpose

The contract through which a licensed third-party translator turns a native CAD file (CATIA, NX, Creo, SolidWorks, Parasolid, JT) into either STEP AP242 bytes, which then go through `IO.CAD.STEP` and `IO.CAD.PMI`, or directly into GEO and QIF entities. No native-format reader ships in the core.

## Scope

**In:**
- The translator descriptor (formats, versions, output kind) registered through `IO.REG.PLUGIN`
- Output validation identical to first-party STEP import
- Provenance: translator plugin ID, version, package digest, and the intermediate STEP digest when used

**Out:**
- Translator implementations (third party)
- Plugin install (ECO, A0), plugin host (`IO.REG.PLUGIN`)

## Requirements

- **REQ-IO.CAD.XLATE-001** The system shall read native CAD formats only through translator plugins registered with `IO.REG`, and the core distribution shall contain no native-format reader.
- **REQ-IO.CAD.XLATE-002** When a translator outputs STEP AP242, the system shall import that output through the same path and checks as a STEP file and record both the native file digest and the intermediate STEP digest.
- **REQ-IO.CAD.XLATE-003** If no translator is installed for a detected native format, then the import shall fail with `translator_missing` naming the format.

## Interfaces

- **Offers:** `import.cad_native@1` (W2), the translator descriptor schema.
- **Consumes:** `IO.REG.PLUGIN` host, `IO.CAD.STEP`, `IO.CAD.PMI`.

## Decomposition hints

DESC (descriptor), BRIDGE (STEP hand-off), DIRECT (direct entity output), after D-019.

## Open questions

- **Unblocked (2026-11-17):** D-019 (Native CAD formats: plugin-only translators or out of scope) is accepted. Apply its Decision section (`decisions/D-019-*.md`) when this node is expanded.
- **Blocked on D-019**: plugin-only translators (as REQ-IO.CAD-011 assumes) versus native CAD out of scope. If D-019 chooses out of scope, this node is removed and the native parts of the covered seeds move to `seeds/out-of-scope.md`.

## Children

_None yet: status `open`._
