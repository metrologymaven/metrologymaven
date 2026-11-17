---
id: IO.DMIS.PARSE
title: Bounded DMIS lexer, parser, syntax tree and printer
level: 3
parent: IO.DMIS
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS-001, REQ-IO.DMIS-002, REQ-IO.DMIS-010]
covers: []
depends_on: [IO.REG.LIMITS]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# IO.DMIS.PARSE: Bounded DMIS lexer, parser, syntax tree and printer

## Purpose

The DMIS 5.3 language front end shared by output-file reading, program import and export: a bounded lexer and parser that produce a typed, lossless syntax tree with source positions, and a printer whose output parses back to the same tree.

## Scope

**In:**
- Lexing: statement continuation (`$`), comments, labels in parentheses, string literals, case rules, line-length bounds
- Grammar coverage for the statements in the declared conformance classes (input program and output file statements)
- Typed syntax tree; every node carries source line and column; unknown statements kept as raw-but-typed `Unknown` nodes
- Printer (tree → text) and the parse → print → parse identity test
- Hooks for dialect profiles (`IO.DMIS.DIAL`)

**Out:**
- Semantic mapping to results (`IO.DMIS.OUT`) and to programs and QIF (`IO.DMIS.PROG`)
- Dialect data (`IO.DMIS.DIAL`)

## Requirements

- **REQ-IO.DMIS.PARSE-001** When a DMIS file is parsed, every syntax-tree node shall carry its source line and column.
- **REQ-IO.DMIS.PARSE-002** For every accepted file in the DMIS corpus, parse → print → parse shall yield an identical syntax tree.
- **REQ-IO.DMIS.PARSE-003** A statement the grammar does not cover shall produce an `Unknown` node holding its keyword and raw tokens, not a parse failure.
- **REQ-IO.DMIS.PARSE-004** If a line exceeds 64 KiB or a continuation chain exceeds 10,000 lines, then parsing shall fail with `limit_exceeded` [unverified: bounds].
- **REQ-IO.DMIS.PARSE-005** The parser shall accept the NIST DMIS Test Suite grammar files that fall inside the declared conformance classes [unverified: the suite targets DMIS 5.2; licence of test files per header].

## Interfaces

- **Offers:** `DmisTree` types, `parse(bytes, dialect?) -> Result<DmisTree>`, `print(&DmisTree) -> String`.
- **Consumes:** `IO.REG.LIMITS`.

## Decomposition hints

LEX, GRAM (grouped by statement family: definitions, measurement, output, motion, control flow), TREE, PRINT.

## Open questions

- Grammar source: built clean-room from the DMIS 5.3 standard's syntax (purchased copy; no text copied) [unverified: access].

## Children

_None yet: status `open`._
