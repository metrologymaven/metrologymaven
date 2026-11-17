---
id: TRUST.VERIFY.WEB
title: In-browser verifier
level: 3
parent: TRUST.VERIFY
kind: module
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY-002]
covers: []
depends_on: [TRUST.VERIFY.REPORT, TRUST.REPLAY.SANDBOX]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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
- **Consumes:** the verifier library crates; web shell conventions from `VIZ.WEB` (not a `depends_on`: VIZ.WEB already depends on TRUST, so the reverse edge would form a cycle).

## Decomposition hints

WASM (library build + parity test), SHELL (page, drop, report view), STREAM (file-API reader adapter), WREPLAY (worker-based replay with limits).

## Open questions

- The UI toolkit for the shell follows D-005 (egui web build) unless a plain static page is simpler; decide at split.
- Nested WASM (the kernel run by an interpreter compiled to WASM) may be slow; measure before promising replay in the browser.

## Children

_None yet: status `open`._
