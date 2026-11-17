---
id: IO.DMIS
title: DMIS 5.3 parsing, import and export
level: 2
parent: IO
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-IO-001, REQ-IO-005]
covers: [PCD.EXP.01]
depends_on: [IO.REG, IO.QIF]
cli: [fid import dmis*, fid export dmis*]
mcp: [import_dmis*, export_dmis*]
journal: [import.dmis*, export.dmis*]
updated: 2026-11-16
---

# IO.DMIS: DMIS 5.3 parsing, import and export

## Purpose

Reads and writes DMIS 5.3 (ANSI; ISO 22093:2011): output files (`.dmo`, measured results) from the start, so that incumbent results can be transcribed in P1a; input programs (`.dmi`) into a typed DMIS program model and QIF features in P2; DMIS output and program export later. It owns the DMIS language and the published conformance table, not execution.

## Scope

**In:**
- A bounded DMIS 5.3 lexer and parser producing a typed syntax tree
- Reading DMIS output files (results) into a typed form for `IO.RES` (P1a)
- Program import into a DMIS program model and QIF features, characteristics and plan items (P2)
- Vendor dialect profiles as data
- Preservation of QPIds carried in DMIS 5.3
- DMIS output export and program export from features and results (later phase)
- The DMIS conformance table: per conformance module and level, whether import and export are supported

**Out** (and which node owns it instead):
- Turning a DMIS program into a Command Program and executing it (CMM)
- Turning DMIS results into transcription records (`IO.RES`)
- Program language features such as loops and variables at run time (AUTO)
- Limits, roots and provenance (`IO.REG`)

## Requirements

- **REQ-IO.DMIS-001** When a DMIS file is parsed, the parser shall produce a typed syntax tree in which every node carries its source line and column.
- **REQ-IO.DMIS-002** For every file in the DMIS corpus, parse → print → parse shall yield an identical syntax tree.
- **REQ-IO.DMIS-003** When a DMIS output file is imported, the system shall extract each reported feature and tolerance result with its label, nominal, actual, deviation, unit and out-of-tolerance flag as written in the file.
- **REQ-IO.DMIS-004** When a DMIS program is imported, every statement the importer does not support shall be listed in the import report with its line number and statement type.
- **REQ-IO.DMIS-005** If `strict` is set and a DMIS program contains an unsupported statement, then the import shall fail with error `dmis_unsupported` and add nothing.
- **REQ-IO.DMIS-006** QPIds present in a DMIS 5.3 file shall be preserved on the created entities and never re-minted.
- **REQ-IO.DMIS-007** When a vendor dialect profile is selected, the parser shall accept that profile's documented deviations and record the profile ID and version in the journal.
- **REQ-IO.DMIS-008** The system shall publish a conformance table, generated from the implementation's declared support, that states for each DMIS 5.3 conformance module and level whether import and export are supported.
- **REQ-IO.DMIS-009** The system shall export DMIS output for features and characteristics to the extent the conformance table declares.
- **REQ-IO.DMIS-010** The parser shall pass the NIST DMIS Test Suite 2.2.1 grammar test files that fall inside the declared conformance classes [unverified: suite targets DMIS 5.2; licence of test files to confirm per header].

## Interfaces

- **Offers:** `import.dmis_output@1` (P1a, results), `import.dmis_program@1` (P2), `export.dmis_output@1` and `export.dmis_program@1` (later); the DMIS syntax-tree and program-model types consumed by CMM and `IO.RES`; the generated conformance table.
- **Consumes:** `IO.REG` harness and provenance; `IO.QIF` feature, characteristic and plan types.

## Decomposition hints

PARSE (lexer, grammar, syntax tree, printer), OUT (output-file reader, P1a), PROG (program import to model and QIF, P2), DIAL (dialect profiles), EXPORT (writers), CONF (conformance table generator).

## Open questions

- Phase: this node is tagged P1a because output-file reading feeds the P1a transcription track; program import is P2 and program export is later (CONSTITUTION §7; concept roadmap puts DMIS export in P4). Children should carry their own phases.
- Is there a DMIS 5.3 conformance test suite, or only the 5.2-era NIST suite? [unverified]

## Children

_None yet: status `open`._
