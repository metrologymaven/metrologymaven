---
id: UNC.RDL.TRACIM.RUN
title: The rdl.submit_tracim@1 command over a protocol trait
level: 4
parent: UNC.RDL.TRACIM
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-UNC.RDL.TRACIM-001, REQ-UNC.RDL.TRACIM-003, REQ-UNC.RDL.TRACIM-004, REQ-UNC.RDL.TRACIM-005]
covers: []
depends_on: [UNC.RDL.TRACIM.ARCH, UNC.RDL.FORMAT, KRN.MAN.KCI, KRN.NUM.FMT.LEN, CORE.BUS.PIPE, CORE.REG.DEF, CORE.BUS.POL.EGRESS]
uses_artifacts: [KRN.MAN.MANI]
side_effect: X1
determinism: N
cli: [fid rdl submit-tracim]
mcp: [rdl_submit_tracim]
journal: [rdl.submit_tracim@1]
updated: 2026-11-17
---

# UNC.RDL.TRACIM.RUN: The rdl.submit_tracim@1 command over a protocol trait

## Purpose

Runs one TraCIM test end to end. It fetches the test data for an order key, converts it to SI with the declared conversion, computes each fit with the kernel, and formats the results back to mm. It then submits them with the software identification `fiducial-kernel <semver> km:sha256:<manifest digest>` and records the returned verdict with that digest. Every exchange goes through the egress primitive and is archived. The protocol details sit behind a `TracimProtocol` trait that `UNC.RDL.TRACIM.PROTO` implements, so this command can be built and tested against a mock service.

## Scope

**In:**
- `TracimProtocol` trait: build fetch request, parse test data into cases (aim, points as decimal text in mm), build submission, parse verdict
- `rdl.submit_tracim@1` (X1, request-only on MCP; data leaves only through `egress.send@1`)
- Order key handling: read from stdin or a CI secret into memory only; never journaled, logged, archived or returned
- Result formatting to mm through the kernel length formatter, with relative error below 1e-15
- Result record: test ID, verdict, report digest, kernel semver and manifest digest

**Out:**
- The real protocol mapping (owned by `UNC.RDL.TRACIM.PROTO`)
- Archive storage (owned by `UNC.RDL.TRACIM.ARCH`)
- Ordering tests and buying keys (a human task)

## Requirements

- **REQ-UNC.RDL.TRACIM.RUN-001** When `rdl.submit_tracim@1` runs, the system shall submit the kernel results with the software identification containing the kernel semver and manifest digest, and journal the returned verdict with that digest and the report digest.
- **REQ-UNC.RDL.TRACIM.RUN-002** Every submitted mm value shall differ from the SI result divided by 1e-3 by a relative error below 1e-15.
- **REQ-UNC.RDL.TRACIM.RUN-003** If the order key is missing or rejected, or the service returns an error or malformed data, then the command shall fail with `tracim_key_invalid`, `tracim_service_error` or `tracim_malformed`, archive the exchange, and send nothing further.
- **REQ-UNC.RDL.TRACIM.RUN-004** The order key shall not appear in the journal, logs, archive, command result or any MCP payload, verified by a test that searches all of them for a sentinel key.
- **REQ-UNC.RDL.TRACIM.RUN-005** Every network exchange shall go through `egress.send@1` to the configured `tracim` destination.

## Acceptance criteria

- **AC-1** Given a mock service from `fixtures/rdl/tracim/mock/` (3 LS cases: plane, circle, cylinder) and sentinel key `KEY-SENTINEL-123`, when a human runs `fid rdl submit-tracim --key-stdin --json`, then results are submitted, the verdict `passed` is journaled with the manifest digest, and the sentinel appears nowhere in journal, logs, archive or output.
- **AC-2** Given the submitted values, then each mm value meets the 1e-15 relative bound against the SI result.
- **AC-3** (error path) Given the mock returning an invalid-key error, then `tracim_key_invalid`, no submission request is sent, and the exchange is archived.
- **AC-4** (error path) Given an agent principal over MCP, then a pending request is filed and the mock receives nothing.

## Design

- **Registry command(s):** `rdl.submit_tracim@1` (X1; params `key_source: stdin | ci_secret`, `test_kind: public | order`; returns `TracimRunRecord`).
- **Data structures:** `TracimProtocol` trait; `TracimCase { aim, points_mm_text }`; `TracimRunRecord { test_id, verdict, report_digest, kernel_semver, manifest_digest, archive_ref }`.
- **Algorithm:** fetch via egress (response archived) -> parse -> convert to SI -> kernel `evaluate` per case with aim -> format to mm -> submit via egress -> parse verdict -> archive -> journal record.
- **Errors:** `tracim_key_invalid`, `tracim_service_error{status}`, `tracim_malformed`, kernel errors per case recorded.
- **Determinism:** N (external service); the kernel results inside are D1.
- **Performance budget:** dominated by the network [unverified].
- **Crate:** `fid-rdl` (module `tracim`).

## Test fixtures

`fixtures/rdl/tracim/mock/`: a mock protocol implementation and loopback server with synthetic cases (CC-BY-4.0); no real TraCIM data.

## Build prompt

> In crate `fid-rdl`, module `tracim`, define trait `TracimProtocol` (fetch request, parse test data into cases with aim and mm decimal points, build submission, parse verdict) and registry command `rdl.submit_tracim@1` (X1, request-only on MCP; CLI `fid rdl submit-tracim`; MCP `rdl_submit_tracim`; journal record digest). Read the order key from stdin or a CI secret into memory only, and never journal, log, archive or return it. Send every exchange through `egress.send@1` to the configured `tracim` destination; archive each exchange through `tracim::archive`. Convert points to SI with the declared conversion, compute with the kernel `evaluate`, format back to mm with the kernel length formatter (relative error < 1e-15), and submit with identification `fiducial-kernel <semver> km:sha256:<manifest digest>`. Journal verdict, report digest, semver and manifest digest. Typed errors `tracim_key_invalid`, `tracim_service_error`, `tracim_malformed`. Tests AC-1..AC-4 with a mock protocol and loopback server. CONSTITUTION 3 (X1), 8.

## Open questions

- Assumes `egress.send@1` returns a bounded response body (cross-domain, see parent).

## Children

_Leaf: none._
