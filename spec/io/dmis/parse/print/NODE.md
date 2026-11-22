---
id: IO.DMIS.PARSE.PRINT
title: DMIS printer and parse-print-parse identity
level: 4
parent: IO.DMIS.PARSE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-io-dmis]
satisfies: [REQ-IO.DMIS.PARSE-002]
covers: []
depends_on: [IO.DMIS.PARSE.TREE]
uses_artifacts: [IO.CORP.MANIF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PARSE.PRINT: DMIS printer and parse-print-parse identity

## Purpose

Prints a DMIS tree back to text in a canonical layout (upper-case words, authored numeric lexemes, one statement per line with continuation when a line would exceed the limit) and proves on the whole DMIS corpus that parse → print → parse gives the identical tree. Exporters use the same printer, so whatever they write parses back.

## Scope

**In:**
- `print(&DmisTree, &PrintOptions) -> String`: canonical spacing, upper-case words, numbers and strings exactly as authored, `Unknown` nodes printed from their raw tokens
- Line wrapping with the continuation marker at a configurable width (default 80 columns, never more than the lexer bound)
- Line endings `\n` or `\r\n` by option
- Identity harness: for each corpus file, parse → print → parse and compare trees ignoring positions

**Out:**
- Tree and parser (`IO.DMIS.PARSE.TREE`); exporters (`IO.DMIS.EXPORT.*`)

## Requirements

- **REQ-IO.DMIS.PARSE.PRINT-001** For every accepted file in the DMIS corpus, parse → print → parse shall yield a tree equal to the first parse except for source positions.
- **REQ-IO.DMIS.PARSE.PRINT-002** The printer shall write numeric and string tokens with their authored spelling and shall never re-format a number.
- **REQ-IO.DMIS.PARSE.PRINT-003** Printed lines shall not exceed the configured width except where a single token is longer, in which case that token shall stand alone on its line.
- **REQ-IO.DMIS.PARSE.PRINT-004** Printing the same tree with the same options twice shall give byte-identical output.

## Acceptance criteria

- **AC-1** Given every `accept` entry of `corpus/dmis.toml` (`IO.CORP.MANIF`), when the identity harness runs, then all trees are equal (positions ignored).
- **AC-2** Given a statement with lexeme `3.000`, then the printed text contains `3.000`.
- **AC-3** Given a 300-column statement and width 80, then all printed lines are ≤ 80 columns and the result parses to the same tree.
- **AC-4** Given a tree with one `Unknown` node, then it prints its raw tokens and re-parses to the same `Unknown` node.

## Design

- **Registry commands:** none (library).
- **Data structures:** `PrintOptions { width: u16, crlf: bool }`.
- **Algorithm:** token-by-token emission with greedy wrapping at argument separators.
- **Errors:** none (printing a valid tree cannot fail).
- **Determinism:** D0.
- **Performance budget:** ≥ 100 MB/s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

`corpus/dmis.toml` (`IO.CORP.MANIF`): project-written programs and output files, plus NIST DMIS Test Suite files whose headers permit redistribution [unverified: licence per file].

## Build prompt

> In crate `fid-io-dmis`, implement `print(&DmisTree, &PrintOptions)`: canonical spacing, upper-case words, numbers and strings with authored spelling (never re-formatted), `Unknown` nodes from raw tokens, wrapping with the continuation marker at a width (default 80) and `\n` or `\r\n`. Add an identity harness over `corpus/dmis.toml`: parse → print → parse must be equal ignoring positions. Byte-identical across runs. CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
