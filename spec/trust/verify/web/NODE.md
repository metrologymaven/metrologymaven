---
id: TRUST.VERIFY.WEB
title: In-browser verifier
level: 3
parent: TRUST.VERIFY
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-002]
covers: []
depends_on: [TRUST.VERIFY.REPORT]
uses_artifacts: [TRUST.PKG.FIXTURE, UX.TOKENS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.VERIFY.WEB: In-browser verifier

## Purpose

A WASM build of the verifier that runs entirely in the user's browser: drop a Result Package (and optionally a trust-root file) on a static page and get the same report as `fid-verify`, with nothing uploaded. It lowers the barrier for customers and auditors who will not install software. It is separate because it adds a browser shell, file-API streaming for large packages, and replay-in-browser limits.

## Scope

**In:**
- wasm32 build of the verifier library (pkg reader, sign core, chain, replay, report)
- A static page shell: file drop, progress, report view, JSON download
- Streaming read of large packages through the browser file API
- Replay with the nested WASM kernel inside a worker, with browser memory limits
- Proof of no upload (no network calls after page load; content security policy)

**Out** (and which node owns it instead):
- Verifier logic (`TRUST.VERIFY.REPORT` and the check leaves)
- Web viewer platform and hosting (`VIZ.WEB`, `OUTR.SITE`)

## Requirements

- **REQ-TRUST.VERIFY.WEB-001** For every golden package, the browser verifier shall produce a report byte-identical to `fid-verify --json` with the same inputs.
- **REQ-TRUST.VERIFY.WEB-002** After the page has loaded, the browser verifier shall make no network request, enforced by a content security policy that forbids connections, and checked by an automated browser test.
- **REQ-TRUST.VERIFY.WEB-003** The browser verifier shall verify (without replay) a 2 GiB package without loading it fully into memory [unverified browser limits].
- **REQ-TRUST.VERIFY.WEB-004** The page shall be served as static files and shall work when saved and opened from local disk.

## Interfaces

- **Offers:** a static web bundle.
- **Consumes:** the verifier library crates (`depends_on`); design tokens from `UX.TOKENS` and golden packages from `TRUST.PKG.FIXTURE` (`uses_artifacts`). No VIZ.WEB code: VIZ.WEB depends on TRUST, and the shell is a plain page, not the viewer.

## Decomposition hints

WASM (library build + parity test), SHELL (page, drop, report view), STREAM (file-API reader adapter), WREPLAY (worker-based replay with limits).

## Open questions

- Shell toolkit (decided at this split): a plain static page whose logic is Rust compiled to wasm32 with DOM access through `web-sys`; no egui and no hand-written JavaScript beyond generated `wasm-bindgen` glue (CONSTITUTION §2 allows TypeScript only for an MCP App host shim). This keeps the bundle small and avoids GPU use in a verifier.
- Nested WASM (the kernel run by the interpreter compiled to WASM) may be slow; `TRUST.VERIFY.WEB.WREPLAY` measures it and the page offers replay only for packages within its budget [unverified].
- Kernel artifacts in the browser: the current major's artifact is shipped in the static bundle and loaded with the page (no fetch after load, REQ-TRUST.VERIFY.WEB-002); an in-package artifact or a user-dropped `.wasm` file are the other sources, mirroring `TRUST.REPLAY.KART.LOOKUP`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.VERIFY.WEB.LIB | wasm32 verifier library build and CLI parity test | 001 | none |
| TRUST.VERIFY.WEB.STREAM | Browser file-API package source | 003 | none |
| TRUST.VERIFY.WEB.WREPLAY | Worker-based replay with browser limits | 001 | none |
| TRUST.VERIFY.WEB.SHELL | Static page shell, offline guarantee and report view | 002 004 | none |
