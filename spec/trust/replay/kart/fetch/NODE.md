---
id: TRUST.REPLAY.KART.FETCH
title: Opt-in kernel artifact fetch into the local cache
level: 4
parent: TRUST.REPLAY.KART
kind: feature
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.REPLAY.KART-002, REQ-TRUST.REPLAY.KART-003]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.EGRESS]
uses_artifacts: [KRN.MAN.MANI.SCHEMA, TRUST.REPLAY.KART.LOOKUP]
side_effect: X1
determinism: N
cli: [fid verify fetch-kernel]
mcp: [verify_fetch_kernel]
journal: [verify.fetch_kernel@1]
updated: 2026-11-17
---

# TRUST.REPLAY.KART.FETCH: Opt-in kernel artifact fetch into the local cache

## Purpose

The only networked part of verifier replay, and it is not in `fid-verify`: the `fid` command `verify.fetch_kernel@1` downloads one WASM kernel artifact, named by its SHA-256, from the project's configured release archive, checks the hash and stores it in the local kernel cache that `TRUST.REPLAY.KART.LOOKUP` reads. It runs only when a person asks for it (D-020 option C). The networked step is class X1 and needs a bus `NetPermit`.

## Scope

**In:**
- Registry command `verify.fetch_kernel@1` (X1), CLI and request-only MCP projection
- Destination taken from configuration, never from the request
- Hash check and atomic write into the cache, named `<sha256>.wasm`
- Journal event with digest, size, destination ID and outcome

**Out:**
- Lookup and use of the cache (`TRUST.REPLAY.KART.LOOKUP`)
- The network guard and permits (`CORE.BUS.POL.EGRESS`)
- Hosting the release archive (`ECO.REL`)

## Requirements

- **REQ-TRUST.REPLAY.KART.FETCH-001** `verify.fetch_kernel@1` shall be class X1, take an artifact SHA-256 and an optional cache directory handle, and open a network connection only under a `NetPermit` issued by the bus for that execution.
- **REQ-TRUST.REPLAY.KART.FETCH-002** The command shall fetch only from the release-archive destination ID configured in settings and shall reject any URL or host in the request with `verify.bad_destination`.
- **REQ-TRUST.REPLAY.KART.FETCH-003** If the downloaded bytes do not hash to the requested SHA-256, or exceed 256 MiB, then the command shall discard them, write nothing and fail with `verify.fetch_mismatch` or `verify.fetch_too_large`.
- **REQ-TRUST.REPLAY.KART.FETCH-004** When the hash matches, the command shall write `<sha256>.wasm` atomically (temporary file, durable flush, rename) into the cache directory and journal one `verify.fetch_kernel@1` event with digest, size, destination ID and outcome.
- **REQ-TRUST.REPLAY.KART.FETCH-005** For `agent` and `script` principals the command shall become a pending request needing human approval, as for every X1 command, and the MCP tool shall be request-only.

## Acceptance criteria

- **AC-1** Given a local test archive server configured as the release destination serving `fixtures/kernel/stub.wasm`, when a human runs `fid verify fetch-kernel <sha> --json`, then the file appears in the cache as `<sha>.wasm`, one journal event is written, and `TRUST.REPLAY.KART.LOOKUP` then returns origin `cache`.
- **AC-2** (error path) Given the server returning altered bytes, then `verify.fetch_mismatch`, no cache file, and an event with outcome `failed`.
- **AC-3** (error path) Given a request carrying a URL parameter, then `verify.bad_destination` and no connection is opened (socket audit).
- **AC-4** Given an MCP agent call, then a pending request is created and nothing is fetched until a human approves.

## Design

- **Registry command(s):** `verify.fetch_kernel@1`, X1; params `sha256: Hex64`, `cache: Option<DirHandle>`; result `{ path_in_cache, size, destination_id }`. CLI `fid verify fetch-kernel`; MCP `verify_fetch_kernel` (request-only).
- **Data structures:** settings key `trust.kernel_archive` (destination ID); URL template `<base>/kernels/<sha256>.wasm` [unverified: archive layout owned by ECO.REL].
- **Algorithm:** resolve destination; GET under `NetPermit` with a size cap; stream-hash; on match write atomically; journal.
- **Errors:** `verify.bad_destination`, `verify.fetch_mismatch`, `verify.fetch_too_large`, `verify.fetch_failed`.
- **Determinism:** N (network observed); the stored bytes are fixed by the hash.
- **Performance budget:** network-bound; no budget.
- **Crate:** `fid-vreplay-fetch` (linked by `fid`, never by `fid-verify`).

## Test fixtures

`fixtures/kernel/stub.wasm` (tiny valid module, project licence) and a loopback test server in the test harness.

## Build prompt

> Create crate `fid-vreplay-fetch` (linked into `fid`, never into `fid-verify`) with registry command `verify.fetch_kernel@1` (X1; CLI `fid verify fetch-kernel <sha256> [--cache DIR]`; MCP `verify_fetch_kernel`, request-only). Destination comes only from settings key `trust.kernel_archive` (reject URLs in params with `verify.bad_destination`). Open the connection only with the bus-issued `NetPermit` for this execution. Download `<base>/kernels/<sha256>.wasm` with a 256 MiB cap (`verify.fetch_too_large`), stream-hash (`verify.fetch_mismatch` on difference, write nothing), then write `<sha256>.wasm` atomically into the kernel cache read by `fid-vreplay::kart`, and journal one event with digest, size, destination ID and outcome (failures included). Agent and script callers get a pending request. Tests: AC-1..AC-4 against a loopback server. Constraints: CONSTITUTION §3 (X1 request-only for agents), §3.1 network rule, §8, D-020 option C.

## Open questions

- Fetching is inbound data, but the request reveals the kernel digest to the archive host; X1 is kept as REQ-TRUST.REPLAY.KART-002 states. Whether the bus primitive `egress.send@1` (send-only) needs a fetch counterpart is for CORE.BUS.POL.EGRESS [cross-domain].

## Children

_Leaf: none._
