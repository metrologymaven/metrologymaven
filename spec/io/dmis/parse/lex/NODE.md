---
id: IO.DMIS.PARSE.LEX
title: Bounded DMIS lexer with continuation, comments and positions
level: 4
parent: IO.DMIS.PARSE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-io-dmis]
built_pr: 147
satisfies: [REQ-IO.DMIS.PARSE-001, REQ-IO.DMIS.PARSE-004]
covers: []
depends_on: [IO.REG.LIMITS]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PARSE.LEX: Bounded DMIS lexer

## Purpose

Turns DMIS bytes into a token stream grouped by logical statement, with the source line and column of every token. It handles statement continuation, comment lines, quoted strings, labels in parentheses and numeric lexemes, and enforces the line-length and continuation bounds, so the grammar never sees unbounded input.

## Scope

**In:**
- Encoding: ASCII and UTF-8 input; any other byte sequence in a string is kept as untrusted bytes, elsewhere it is `malformed`
- Logical statements: physical lines joined across the continuation marker `$` at line end [unverified: exact continuation rule to confirm against the DMIS 5.3 syntax]; comment lines starting with `$$`
- Tokens: words (case-insensitive, normalised to upper case, original spelling kept), labels `F(...)`, `FA(...)`, `T(...)` and other label prefixes, `/`, `,`, `=`, `(`, `)`, numbers with their authored lexeme, quoted strings with doubled-quote escapes, and the remainder of a `TEXT` or comment line as raw text
- Positions: 1-based line and column of each token's first byte in the physical file
- Bounds: line ≤ 64 KiB, continuation chain ≤ 10,000 lines, token count per statement ≤ 10^5 [unverified: bounds]

**Out:**
- Statement grammar and tree (`IO.DMIS.PARSE.TREE`)
- Dialect lexical relaxations: supplied as a `LexOptions` value by `IO.DMIS.DIAL`

## Requirements

- **REQ-IO.DMIS.PARSE.LEX-001** Every token shall carry the 1-based physical line and column of its first byte.
- **REQ-IO.DMIS.PARSE.LEX-002** If a physical line exceeds 64 KiB or a continuation chain exceeds 10,000 lines, then lexing shall fail with `limit_exceeded` naming the bound and the starting line.
- **REQ-IO.DMIS.PARSE.LEX-003** If a quoted string is not closed before the end of its logical statement, then lexing shall fail with `malformed{line, col, reason: unterminated_string}`.
- **REQ-IO.DMIS.PARSE.LEX-004** Numeric tokens shall keep their authored lexeme exactly, and string and comment text shall be returned as untrusted text.
- **REQ-IO.DMIS.PARSE.LEX-005** When `LexOptions` from a dialect enable a relaxation, the lexer shall apply only that relaxation.

## Acceptance criteria

- **AC-1** Given `fixtures/io/dmis/parse/lex/continued.dmi` (a statement continued over 3 lines, a comment line, a string with a doubled quote), when lexed, then one logical statement is returned whose tokens carry the expected lines and columns, the comment is a comment token and the string decodes correctly.
- **AC-2** Given `F(P1)=FEAT/POINT,CART,1.50,2,3.000,0,0,1`, then numeric lexemes are `1.50`, `2`, `3.000` exactly.
- **AC-3** (error path) Given a generated 65 KiB line, then `limit_exceeded{bound: line_length, line: 1}`.
- **AC-4** (error path) Given `fixtures/io/dmis/parse/lex/open_string.dmi`, then `malformed{reason: unterminated_string}` with its line and column.
- **AC-5** Given the lexer fuzz target run for its CI budget, then no panic occurs.

## Design

- **Registry commands:** none (library).
- **Data structures:** `Token { kind, text: Lexeme, pos: Pos { line: u32, col: u32 } }`; `LogicalStatement { tokens: Vec<Token>, first_line, last_line }`; `LexOptions { relaxations: BTreeSet<Relaxation> }`.
- **Algorithm:** byte-level state machine over a `BoundedSource`, one logical statement at a time.
- **Errors:** `malformed{line, col, reason}`, `limit_exceeded{bound, line}`.
- **Determinism:** D0.
- **Performance budget:** ≥ 100 MB/s [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Hand-written DMIS fragments under `fixtures/io/dmis/parse/lex/` (CC-BY-4.0), written by the project, not copied from the standard.

## Build prompt

> In crate `fid-io-dmis`, implement a bounded DMIS lexer: join physical lines into logical statements across the continuation marker, recognise `$$` comment lines, case-insensitive words (keep original spelling), labels with prefixes, punctuation `/ , = ( )`, numbers with exact authored lexemes, quoted strings (doubled-quote escape; unterminated → `malformed{reason: unterminated_string}`), and raw text for `TEXT` and comments (untrusted). Every token carries 1-based line and column. Bounds: line 64 KiB, continuation 10,000 lines, 10^5 tokens per statement → `limit_exceeded{bound, line}`. Accept `LexOptions` relaxations from dialect profiles. Use `fid-io-reg` `BoundedSource`. No standard text in code or fixtures. CONSTITUTION §8: no panic, no unwrap. Fuzz target. Tests AC-1..AC-5.

## Open questions

- Exact continuation and comment rules to confirm against DMIS 5.3 (purchased copy, no text copied) [unverified].

## Children

_Leaf: none._
