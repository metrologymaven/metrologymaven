---
id: IO.REG.PLUGIN.NATIVE
title: Out-of-process host for native plugins
level: 4
parent: IO.REG.PLUGIN
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.REG.PLUGIN-002, REQ-IO.REG.PLUGIN-003]
covers: []
depends_on: [IO.REG.PLUGIN.ABI, IO.REG.LIMITS]
side_effect: R1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.REG.PLUGIN.NATIVE: Out-of-process host for native plugins

## Purpose

Native translator SDKs (D-019: native CAD formats only through licensed plugins) usually cannot compile to WASM. This leaf runs such plugins in a separate worker process per invocation, with OS memory and time caps, no network, and no file access beyond the bytes it is handed, speaking the contract's message protocol. A crash or hang in vendor code fails one import, not the app.

## Scope

**In:**
- Worker launcher: spawn a small `fid-plugin-worker` executable that loads the plugin library, handshake with contract version check, one request, response stream, exit
- OS caps: Windows job objects (memory, process count, kill on close), Linux rlimits plus a seccomp or namespace profile, macOS sandbox profile [unverified: exact mechanism per OS]
- No network, no inherited handles, a private empty working directory, environment cleared
- Kill on wall-time cap; mapping exits and timeouts to typed errors

**Out:**
- The contract (`IO.REG.PLUGIN.ABI`) and output validation (`IO.REG.PLUGIN.VALID`)
- The CAD kernel worker for first-party STEP/IGES (`IO.CAD.WORKER`, which may reuse this launcher; see open questions)

## Requirements

- **REQ-IO.REG.PLUGIN.NATIVE-001** The worker shall receive the source only as bytes over its message channel and shall start with no inherited handles other than that channel, an empty working directory and a cleared environment.
- **REQ-IO.REG.PLUGIN.NATIVE-002** If the worker exits abnormally or sends a malformed message, then the import shall fail with `reader_crashed` and the document shall be unchanged.
- **REQ-IO.REG.PLUGIN.NATIVE-003** If the worker exceeds its memory cap or wall-time cap, then the host shall terminate it and fail the import with `limit_exceeded` naming `plugin_memory` or `plugin_wall_time`.
- **REQ-IO.REG.PLUGIN.NATIVE-004** If the worker attempts a network connection, then the attempt shall fail inside the worker, verified by a test plugin on each Tier 1 OS.
- **REQ-IO.REG.PLUGIN.NATIVE-005** Every message from the worker shall be length-prefixed, at most 64 MiB [unverified: tune], and parsed under `IO.REG.LIMITS`.

## Acceptance criteria

- **AC-1** Given `fixtures/io/reg/plugin/native/sample_reader` (built in CI) and `pts.txt`, when imported, then the import succeeds and provenance names the plugin.
- **AC-2** (error path) Given `segv_reader` that dereferences null, then `reader_crashed`, document unchanged, app still running.
- **AC-3** (error path) Given `hog_reader` allocating past the cap, then `limit_exceeded` (`plugin_memory`).
- **AC-4** (error path) Given `sleep_reader`, then `limit_exceeded` (`plugin_wall_time`) at the cap ± 1 s.
- **AC-5** Given `net_reader` that tries to open a TCP connection and reports the result, then the reported result is failure on Windows, Linux and macOS.
- **AC-6** (error path) Given `junk_reader` that writes a 2 GiB length prefix, then `reader_crashed` without allocating it.

## Design

- **Registry commands:** none (host library).
- **Data structures:** `NativeHost { worker_exe, caps }`; `WorkerSession`; messages from the contract's generated schema.
- **Algorithm:** spawn worker with OS caps; handshake (contract version); send request and stream bytes; read length-prefixed records with a budget; on timeout kill the process tree; wait and classify exit.
- **Errors:** `reader_crashed`, `limit_exceeded`, `plugin_abi_mismatch`.
- **Determinism:** N (process scheduling and wall-time kills); committed records are still validated by `IO.REG.PLUGIN.VALID`.
- **Performance budget:** spawn and handshake < 200 ms [unverified until measured].
- **Crate:** `fid-io-plugin` (module `native`) and binary `fid-plugin-worker`. OS sandbox calls need a vetted safe wrapper crate or a named `unsafe` exception (CONSTITUTION §8).

## Test fixtures

Hostile and sample native test plugins built from source in `fixtures/io/reg/plugin/native/src/` (MIT OR Apache-2.0).

## Build prompt

> In crate `fid-io-plugin`, add module `native` and the `fid-plugin-worker` binary: spawn one worker per invocation with an empty working directory, cleared environment, no inherited handles but the message channel, OS memory and process caps (job objects on Windows, rlimits plus seccomp or namespaces on Linux, sandbox profile on macOS) and no network; handshake on contract version; stream source bytes; read length-prefixed messages (≤ 64 MiB, parsed under the limit budget); kill on wall-time cap. Map abnormal exits and malformed messages to `reader_crashed`, cap overruns to `limit_exceeded` (`plugin_memory`/`plugin_wall_time`). Any `unsafe` only behind a named exception. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-6.

## Open questions

- `IO.CAD.WORKER` specifies its own isolated process for the CAD kernel with the same caps. The two should share one launcher; this leaf offers it, and `IO.CAD.WORKER` would need to depend on it (cross-domain note for IO.CAD's owner).

## Children

_Leaf: none._
