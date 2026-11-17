---
id: UNC.RDL.TRACIM
title: PTB TraCIM client
level: 3
parent: UNC.RDL
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL-007]
covers: []
depends_on: [UNC.RDL.FORMAT, KRN]
cli: [fid rdl submit-tracim]
mcp: [rdl_submit_tracim]
journal: [rdl.submit_tracim@1]
updated: 2026-11-16
---

# UNC.RDL.TRACIM: PTB TraCIM client

## Purpose

A client for PTB's TraCIM algorithm-testing service: fetch test data with an order key, compute results with the kernel, submit them with the software identification (kernel semver and manifest digest), and store the returned verdict or report verbatim. The free public test runs continuously and validates client and kernel without a certificate; paid certificated tests are ordered by a human when fits freeze. Data leaves the machine, so submission is X1.

## Scope

**In:**
- REST/HTTPS client for the TraCIM test workflow (order key, fetch XML test data, submit results, receive report)
- Conversion of TraCIM mm data to SI with declared conversion, and of results back to mm for submission
- Software identification string `fiducial-kernel <semver> km:sha256:<manifest digest>`
- Storing returned reports verbatim with digest; archiving everything sent and received
- `rdl.submit_tracim@1` (X1, request-only on MCP)

**Out:**
- Buying tests and managing order keys (a human task)
- Publishing claims (owned by `KRN.MAN` and `OUTR` claim linting)

## Requirements

- **REQ-UNC.RDL.TRACIM-001** When a TraCIM test is run, the client shall submit the kernel results with the software identification containing the kernel semver and manifest digest, and shall record the returned verdict with that digest.
- **REQ-UNC.RDL.TRACIM-002** The client shall archive every request and response with its digest and store returned reports byte for byte.
- **REQ-UNC.RDL.TRACIM-003** Results submitted in mm shall be formatted back from SI with relative error below 1e-15.
- **REQ-UNC.RDL.TRACIM-004** If the order key is missing or invalid, or the service returns an error, then the client shall fail with a typed error and submit nothing further.
- **REQ-UNC.RDL.TRACIM-005** Order keys shall be treated as secrets: never journaled, logged or sent to an MCP client.

## Interfaces

- **Offers:** `rdl.submit_tracim@1`; stored verdicts and reports for `KRN.MAN` (REQ-KRN.MAN-008).
- **Consumes:** kernel fits through the bus; dataset format for archived test data (UNC.RDL.FORMAT).

## Decomposition hints

PROTO (protocol and XML mapping), RUN (compute and submit), ARCHIVE (verbatim storage), PUBLIC (nightly public-test job).

## Open questions

- The TraCIM REST and XML schemas have not been inspected for this spec [unverified].
- Whether automated CI submission of paid orders is acceptable must be asked of PTB [unverified].
- Redistribution of TraCIM test data and reports is limited to verbatim copies [unverified]; archived test data is `reference_only`.

## Children

_None yet: status `open`._
