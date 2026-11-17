---
id: TRUST.VERIFY.WEB.WREPLAY
title: Worker-based replay with browser limits
level: 4
parent: TRUST.VERIFY.WEB
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.VERIFY.WEB-001]
covers: []
depends_on: [TRUST.VERIFY.WEB.LIB, TRUST.REPLAY.RUN, TRUST.REPLAY.SANDBOX]
uses_artifacts: [KRN.MAN.WASM, TRUST.PKG.FIXTURE]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.VERIFY.WEB.WREPLAY: Worker-based replay with browser limits

## Purpose

Runs verifier replay in the browser: the `fid-vreplay` pipeline, including its interpreting sandbox, runs inside a dedicated Web Worker so the page stays responsive, with a memory cap and time budget sized for browsers. Kernel artifacts come from the static bundle, the package, or a file the user drops. Its replay section must equal the CLI's for the replay golden packages.

## Scope

**In:**
- Worker entry point and message protocol (start, progress, result, cancel)
- Browser `ArtifactSource`: bundled static artifact, in-package member, user-dropped `.wasm`
- Browser limits: memory cap (default 1 GiB) and time budget (default 10 min)
- Replay parity test against the CLI

**Out:**
- Replay logic and the sandbox (`TRUST.REPLAY.RUN`, `TRUST.REPLAY.SANDBOX`)
- Native lookup chain and cache (`TRUST.REPLAY.KART.LOOKUP`)
- The page controls (`TRUST.VERIFY.WEB.SHELL`)

## Requirements

- **REQ-TRUST.VERIFY.WEB.WREPLAY-001** For every replay golden package, the browser replay section shall equal `fid-verify --replay --json`'s replay section, apart from the kernel origin, which shall be `bundled`, `package` or `user_file`.
- **REQ-TRUST.VERIFY.WEB.WREPLAY-002** Replay shall run in a dedicated Web Worker, and cancel shall stop it within 1 s and report `replay.status` `cancelled` [unverified: worker termination latency].
- **REQ-TRUST.VERIFY.WEB.WREPLAY-003** The worker shall pass a memory cap (default 1 GiB) and time budget (default 600 s) to the sandbox, and exceeding either shall yield `NOT_REPLAYABLE` with reason `resource`, never a tab crash.
- **REQ-TRUST.VERIFY.WEB.WREPLAY-004** The worker shall obtain kernel artifacts only from the bundle loaded with the page, the package, or a user-dropped file, and shall make no network request.

## Acceptance criteria

- **AC-1** Given replay golden packages `fixtures/pkg/golden/points*.fidpkg`, when replayed in headless Chromium and Firefox, then each replay section equals the CLI's except the origin field.
- **AC-2** Given a package whose kernel is neither bundled nor included and no dropped file, then `NOT_REPLAYABLE(KERNEL_UNAVAILABLE)`; after dropping the matching `.wasm`, origin is `user_file` and the outcome equals the CLI's.
- **AC-3** (error path) Given a memory cap of 64 MiB on a large package, then `NOT_REPLAYABLE(resource)` and the page remains responsive.
- **AC-4** Given a cancel during replay, then `cancelled` within 1 s.

## Design

- **Registry command(s):** none.
- **Data structures:** worker messages `Start { package_ref, opts }`, `Progress { done, total }`, `Done { replay_json }`, `Cancel`; `BrowserArtifactSource`.
- **Algorithm:** the page transfers the `File` handle to the worker; the worker builds the `BlobSource`, runs `fid-vreplay::run` with the browser source and limits; cancel terminates the worker.
- **Errors:** as in `verify.replay@1`, plus `cancelled`.
- **Determinism:** D0 (replay outcomes must match the CLI).
- **Performance budget:** to be measured per golden package; interpreter-in-WASM slowdown expected at 10x to 50x native [unverified].
- **Crate:** `fid-verify-web`, module `wreplay` (worker entry).

## Test fixtures

Replay golden packages from `TRUST.PKG.FIXTURE`/`TRUST.REPLAY.CORPUS`; kernel artifact from the `KRN.MAN.WASM` build.

## Build prompt

> In crate `fid-verify-web`, add module `wreplay`: a Web Worker entry running `fid-vreplay::run` (with its interpreting sandbox) on a package read through `BlobSource`. Implement `BrowserArtifactSource` (bundled artifact embedded in the page files and instantiated from bytes, in-package `kernel/<sha256>.wasm`, or a user-dropped file; origins `bundled|package|user_file`; never fetch). Defaults: memory cap 1 GiB, time budget 600 s, both overridable; over-limit → `NOT_REPLAYABLE(resource)`. Message protocol Start/Progress/Done/Cancel; cancel terminates the worker and reports `cancelled`. Tests: AC-1..AC-4 headless, comparing with `fid-verify --replay --json`. Constraints: REQ-TRUST.REPLAY-007/009 (sandbox, hash check), REQ-TRUST.VERIFY.WEB-002 (no network), CONSTITUTION §8.

## Open questions

- If nested interpretation is too slow for typical packages, the page should say so plainly rather than time out silently (honest status).

## Children

_Leaf: none._
