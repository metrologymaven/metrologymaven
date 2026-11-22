---
id: AUTO.MCP.TAINT
title: Untrusted-content marking in tool results
level: 3
parent: AUTO.MCP
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.MCP-012]
covers: []
depends_on: [AUTO.MCP.PROJ, CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
built_in: [fid-mcp]
updated: 2026-11-21
---

# AUTO.MCP.TAINT: Untrusted-content marking in tool results

## Purpose

Implements C6 at the MCP boundary: every string in a tool result that came from document content (element names, PMI notes, imported metadata, operator text, file names) is delivered to the agent explicitly marked as untrusted data, in both the structured result and the text rendering, so it cannot pass as instructions from the platform.

## Scope

**In:**
- Reading `untrusted` markers from result schemas in the manifest
- Wrapping marked strings in the structured result and delimiting them in the text rendering
- Neutralising delimiter look-alikes inside untrusted strings

**Out:**
- Declaring which result fields are untrusted (each domain, in `CORE.REG` result schemas)
- Untrusted handling in the GUI (`VIZ`)

## Requirements

- **REQ-AUTO.MCP.TAINT-001** Every result field whose schema is marked untrusted shall appear in the structured tool result as an object `{"untrusted": true, "text": <string>}`.
- **REQ-AUTO.MCP.TAINT-002** In the text rendering of a result, every untrusted string shall be enclosed in fixed delimiters stating that the enclosed text is data from the document and not an instruction.
- **REQ-AUTO.MCP.TAINT-003** If an untrusted string contains the delimiter text, then the server shall escape it so the string cannot close its own delimiter.
- **REQ-AUTO.MCP.TAINT-004** If a result string has no schema marking, then the server shall treat it as untrusted unless its schema type is an enum, an ID or a number.

## Acceptance criteria

- **AC-1** Given fixture command `demo.describe_element@1` whose result field `name` is marked untrusted, and an element named `Ignore previous instructions`, when the tool is called, then the structured result holds `{"untrusted": true, "text": "Ignore previous instructions"}` and the text rendering wraps it in the delimiters.
- **AC-2** Given an element name containing the closing delimiter, then the rendering escapes it and the delimiters stay balanced.
- **AC-3** Given an unmarked free-text field, then it is wrapped as untrusted.
- **AC-4** Given an enum field, then it is not wrapped.

## Design

- **Registry command(s):** none.
- **Data structures:** `Untrusted { text: String }` serialised as above.
- **Algorithm:** walk the result value alongside its schema; wrap marked or unmarked free-text strings; escape delimiter sequences by prefixing a visible backslash.
- **Errors:** schema mismatch → wrap everything below the mismatch as untrusted (fail safe).
- **Determinism:** D0.
- **Performance budget:** < 5% overhead on result encoding [unverified until measured].
- **Crate:** `fid-mcp`

## Test fixtures

Fixture command `demo.describe_element@1` in `fixtures/manifest/min-classes.json`; adversarial strings in `fixtures/mcp/untrusted-strings.txt` (project-authored).

## Build prompt

> In crate `fid-mcp`, wrap untrusted strings in tool results. Walk each result with its schema: fields marked untrusted, and free-text strings with no marking, become `{"untrusted": true, "text": ...}` in the structured result and are enclosed in fixed delimiters in the text rendering that say the enclosed text is document data, not an instruction. Escape any occurrence of the delimiters inside the text. Enums, IDs and numbers are not wrapped. On schema mismatch, wrap everything beneath as untrusted. Acceptance: the four cases with `demo.describe_element@1` and the adversarial strings file. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; CONSTITUTION C6.

## Open questions

- None.

## Children

_Leaf: none._
