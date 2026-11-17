---
id: IO.DMIS.PARSE.TREE
title: Statement tree, descriptor registry, Unknown nodes and statement-class list
level: 4
parent: IO.DMIS.PARSE
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.DMIS.PARSE-001, REQ-IO.DMIS.PARSE-003]
covers: []
depends_on: [IO.DMIS.PARSE.LEX]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.DMIS.PARSE.TREE: Statement tree, descriptor registry and Unknown nodes

## Purpose

The table-driven core of the DMIS parser. Each statement family registers descriptors (major word, label kind, allowed minor words and argument shapes); the parser turns each logical statement into a lossless generic `Statement` node checked against its descriptor, or into an `Unknown` node holding the keyword and raw tokens when no descriptor matches. Typed views over the generic nodes are supplied by the grammar families. It also publishes the statement-class list that other nodes key tables on.

## Scope

**In:**
- `DmisTree { statements: Vec<Statement> }`; `Statement { pos, label: Option<Label>, major: Word, args: Vec<Arg>, class: StatementClass }` with `Arg` = word, number (lexeme), string, label reference, nested group; `Unknown { pos, keyword, tokens }`
- `StatementDescriptor { class, major, label_kind, shape }` with a small shape language (sequences, alternatives, optional groups, repeats with bounds)
- Descriptor registry: families (`IO.DMIS.PARSE.GDEF`, `IO.DMIS.PARSE.GEXEC`) and dialect profiles register descriptors; duplicates rejected at build time
- `parse(bytes, dialect?) -> Result<DmisTree>`: shape mismatch on a known major word → typed `malformed` with position; unknown major word → `Unknown` node
- Statement-class list artifact `dmis-statement-classes.json` (schema `fid.dmis-classes` v1: class ID, major word, label kind, family, phase), generated from the registry
- Dialect hook: a `Dialect` value adds descriptors and `LexOptions`

**Out:**
- Lexing (`IO.DMIS.PARSE.LEX`), printing (`IO.DMIS.PARSE.PRINT`), concrete descriptors (`.GDEF`, `.GEXEC`), dialect data (`IO.DMIS.DIAL`)

## Requirements

- **REQ-IO.DMIS.PARSE.TREE-001** Every `Statement` and `Unknown` node shall carry the line and column of its first token.
- **REQ-IO.DMIS.PARSE.TREE-002** A statement whose major word has no registered descriptor shall produce an `Unknown` node holding its keyword and raw tokens, and parsing shall continue.
- **REQ-IO.DMIS.PARSE.TREE-003** If a statement's major word is registered but its arguments do not match the descriptor's shape, then parsing shall fail with `malformed{line, col, class}`, unless `lenient` is set, in which case the statement shall become an `Unknown` node with reason `shape_mismatch`.
- **REQ-IO.DMIS.PARSE.TREE-004** The generic tree shall be lossless: every token of the input except whitespace and continuation markers shall be recoverable from it, with its authored spelling.
- **REQ-IO.DMIS.PARSE.TREE-005** The build shall generate `dmis-statement-classes.json` from the registry, byte-identical for the same registry, and CI shall fail if the committed file differs.

## Acceptance criteria

- **AC-1** Given a registry with a test descriptor for `FEAT/POINT` and `fixtures/io/dmis/parse/tree/mixed.dmi` (one point definition and one `FOO/1,2` statement), when parsed, then one `Statement` of class `feat_point` and one `Unknown{keyword: FOO}` exist with positions.
- **AC-2** (error path) Given `F(P1)=FEAT/POINT,CART,1,2` (too few arguments), then `malformed{class: feat_point}` with line and column; with `lenient`, an `Unknown{reason: shape_mismatch}`.
- **AC-3** Given any fixture, then concatenating the tree's tokens' authored spellings reproduces the input's token sequence.
- **AC-4** Given the registry, then `dmis-statement-classes.json` is generated and two generations are byte-identical.
- **AC-5** Given a dialect adding descriptor `VENDOR/X`, then a statement `VENDOR/X,1` parses as a `Statement` only when that dialect is selected.

## Design

- **Registry commands:** none (library + build-time generator).
- **Data structures:** as in Scope; `Registry { by_major: BTreeMap<Word, Vec<StatementDescriptor>> }`.
- **Algorithm:** for each logical statement: split label assignment (`label = major/...`), look up major word, match shape by bounded backtracking (≤ 10^4 steps per statement), build node.
- **Errors:** `malformed{line, col, class}`, `limit_exceeded`.
- **Determinism:** D0.
- **Performance budget:** ≥ 50 MB/s including lexing [unverified until measured].
- **Crate:** `fid-io-dmis`.

## Test fixtures

Hand-written fragments under `fixtures/io/dmis/parse/tree/` (CC-BY-4.0).

## Build prompt

> In crate `fid-io-dmis`, implement the table-driven DMIS statement tree: generic lossless `Statement { pos, label, major, args, class }` and `Unknown { pos, keyword, tokens, reason? }` nodes; a descriptor registry (major word, label kind, argument-shape language with sequences, alternatives, optionals and bounded repeats; duplicates rejected) filled by grammar families and dialects; `parse(bytes, dialect?)` that yields `Unknown` for unregistered majors, `malformed{line, col, class}` for shape mismatches (or `Unknown{reason: shape_mismatch}` when `lenient`), with bounded backtracking. Generate `dmis-statement-classes.json` (schema `fid.dmis-classes` v1: class, major, label kind, family, phase; RFC 8785) from the registry and add a CI diff check. No standard text. CONSTITUTION §8: no panic, no unwrap; BTreeMap only. Tests AC-1..AC-5.

## Open questions

- None.

## Children

_Leaf: none._
