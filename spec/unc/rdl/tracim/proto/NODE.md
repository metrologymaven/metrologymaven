---
id: UNC.RDL.TRACIM.PROTO
title: TraCIM protocol and XML mapping
level: 4
parent: UNC.RDL.TRACIM
kind: task
status: open
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.TRACIM-001, REQ-UNC.RDL.TRACIM-004]
covers: []
depends_on: [UNC.RDL.TRACIM.RUN]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.RDL.TRACIM.PROTO: TraCIM protocol and XML mapping

## Purpose

The real implementation of `TracimProtocol` for PTB's TraCIM service: request shapes, the XML test-data and result schemas, error responses, and which fields are secret. It stays open because the schemas have not been inspected for this spec.

## Scope

**In:**
- Protocol card written in our own words from PTB's public documentation
- Bounded XML parsing of test data and verdict reports; result XML writer
- Mapping of TraCIM test types to kernel aims; secret-span marking

**Out:**
- The command and its trait (owned by `UNC.RDL.TRACIM.RUN`)

## Requirements

- **REQ-UNC.RDL.TRACIM.PROTO-001** The implementation shall map each supported TraCIM test type to one kernel aim and parse its test data into cases with mm decimal text.
- **REQ-UNC.RDL.TRACIM.PROTO-002** If a response deviates from the protocol card or exceeds parser bounds, then parsing shall fail with `tracim_malformed`.
- **REQ-UNC.RDL.TRACIM.PROTO-003** The implementation shall mark every field that carries the order key as a secret span.

## Interfaces

- **Offers:** `TracimProtocol` implementation.
- **Consumes:** the trait (UNC.RDL.TRACIM.RUN).

## Decomposition hints

Likely one leaf once a person has read the TraCIM documentation and recorded sample exchanges from the public test.

## Open questions

- Schemas unverified [unverified]; the terms for reusing sample exchanges as fixtures must be checked.

## Children

_None yet: status `open`._
