---
id: IO.CAD.WORKER.HOST
title: Worker spawning, OS caps, kill on limit and failure mapping
level: 4
parent: IO.CAD.WORKER
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.WORKER-001, REQ-IO.CAD.WORKER-002, REQ-IO.CAD.WORKER-005]
covers: []
depends_on: [IO.CAD.WORKER.PROTO, IO.REG.LIMITS, IO.REG.PROV]
side_effect: R1
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.WORKER.HOST: Worker spawning, OS caps, kill on limit and failure mapping

## Purpose

The document-process side of CAD reader isolation: the `CadWorker` client that starts one worker process per import, applies memory and wall-time caps with the operating system's own mechanisms, performs the handshake, streams the request and records, and maps every abnormal outcome (crash, hang, memory blow-up, bad message) to `reader_crashed` or `limit_exceeded` before anything reaches the document.

## Scope

**In:**
- Locating the `fid-cad-worker` executable next to the running binary (never from `PATH`), and checking its file digest against the build-time digest
- Spawning with an empty environment, stdin/stdout pipes, stderr captured to a bounded 64 KiB buffer
- Memory caps: Windows job object with a process memory limit; Linux `RLIMIT_AS` plus a cgroup v2 limit when available; macOS `RLIMIT_AS` or the closest available mechanism [unverified: macOS enforcement]
- Wall-time cap with kill; cancellation from the CORE job token kills the worker
- Handshake through `IO.CAD.WORKER.PROTO`; recording the worker's reader name and version for provenance
- Mapping: abnormal exit → `reader_crashed{reason: exited, code}`; cap hit → `limit_exceeded{cap: memory|wall}`; protocol failure → `reader_crashed{...}`
- `CadWorker` client API: `load_step(bytes, opts)`, `load_iges(bytes, opts)` returning a validated record stream and report

**Out:**
- The protocol encoding (`IO.CAD.WORKER.PROTO`)
- Sandbox self-restriction inside the worker (`IO.CAD.WORKER.BIN`)
- Turning records into document entities (`IO.CAD.STEP.CMD`, `IO.CAD.IGES.CMD`)

## Requirements

- **REQ-IO.CAD.WORKER.HOST-001** If the worker process exits before sending its final `Report`, then the client shall return `reader_crashed{reason: exited}` with the exit code and the first 4 KiB of captured stderr as untrusted text.
- **REQ-IO.CAD.WORKER.HOST-002** If the worker exceeds the memory cap (default 4 GiB) or the wall-time cap (default 600 s), then the host shall kill it and return `limit_exceeded` naming the cap.
- **REQ-IO.CAD.WORKER.HOST-003** When the import's job is cancelled, the host shall kill the worker within 1 s and return `cancelled`.
- **REQ-IO.CAD.WORKER.HOST-004** If the worker executable's digest differs from the digest recorded at build time, then the host shall refuse to start it and return `reader_crashed{reason: worker_digest_mismatch}`.
- **REQ-IO.CAD.WORKER.HOST-005** When a load completes, the client shall return the worker's reader name and version from the handshake for recording as the reader version in the import provenance.
- **REQ-IO.CAD.WORKER.HOST-006** The client shall never return records to its caller until they have passed protocol validation, so a failing load leaves its caller with no partial records.

## Acceptance criteria

- **AC-1** Given the test worker `fid-cad-worker-test` in mode `crash` (fixture binary built from `fixtures/io/cad/worker/host/`), when `load_step` runs, then the result is `reader_crashed{reason: exited}` and no records are returned.
- **AC-2** Given mode `alloc_forever` and a 256 MiB memory cap, then the result is `limit_exceeded{cap: memory}` within 10 s on Windows, Linux and macOS [unverified on macOS].
- **AC-3** Given mode `hang` and a 2 s wall cap, then the result is `limit_exceeded{cap: wall}` and the process no longer exists 1 s later.
- **AC-4** Given mode `hang` and a cancelled job token after 0.5 s, then the result is `cancelled` and the process is gone within 1 s.
- **AC-5** (error path) Given a worker executable with one byte changed, then the result is `reader_crashed{reason: worker_digest_mismatch}` and no process is started.
- **AC-6** Given mode `echo` (valid handshake `reader: test, version 0.1.0`), then the client returns the reader name and version.

## Design

- **Registry commands:** none (library used by `import.cad_step@1`, `import.cad_iges@1`).
- **Data structures:** `CadWorker { exe: PathBuf, digest: [u8; 32], caps: WorkerCaps }`; `WorkerCaps { memory_bytes, wall: Duration, proto: ProtoLimits }`; `LoadOutcome { records: RecordStream, report: WorkerReport, reader: ReaderId, reader_version }`.
- **Algorithm:** verify digest → spawn suspended where the OS allows it, assign to job object or set rlimits → resume → handshake → write request → read frames on a reader thread with a monotonic deadline → join or kill. Caps come from `IO.REG.LIMITS` defaults and can only be lowered by non-human principals.
- **Errors:** `reader_crashed{reason}`, `limit_exceeded{cap}`, `cancelled`.
- **Determinism:** D1: the outcome for a given input and caps is the same per platform, except wall-time expiry, which is `N` and reported as such.
- **Performance budget:** spawn + handshake < 150 ms [unverified until measured].
- **Crate:** `fid-cad-host`. Process-control calls go through safe wrapper crates on the D-001 allowlist; any `unsafe` needs a named exception (CONSTITUTION §8).

## Test fixtures

A tiny test worker binary with modes `crash`, `hang`, `alloc_forever`, `echo`, `garbage` under `fixtures/io/cad/worker/host/` (MIT OR Apache-2.0, project code).

## Build prompt

> Create crate `fid-cad-host` with `CadWorker`, the document-side client for the CAD reader worker. Locate `fid-cad-worker` beside the current executable (never via PATH), verify its SHA-256 against the build-time digest (`reader_crashed{reason: worker_digest_mismatch}`), spawn it with an empty environment and piped stdio, apply a memory cap (Windows job object; Linux RLIMIT_AS plus cgroup v2 when present; macOS best available, document the gap) and a wall-time deadline; kill on cap → `limit_exceeded{cap}`, on cancellation (CORE job token) → `cancelled` within 1 s, on abnormal exit → `reader_crashed{reason: exited, code, stderr_head}` (stderr is untrusted text, 4 KiB). Speak `fid-cad-proto` (handshake, request, validated `Records` stream, `Report`); return records only after validation; return reader name and version for provenance (`IO.REG.PROV`). CONSTITUTION §8: no panic, no unwrap; no `unsafe` without a named exception. Tests AC-1..AC-6 with a test worker binary.

## Open questions

- macOS memory-cap enforcement for child processes is weaker than job objects or cgroups [unverified]; the leaf documents the gap rather than claiming parity.
- `IO.REG.PLUGIN` will need an out-of-process host for native translator plugins (D-019, D-024 option C); it may reuse this crate. Recorded here as a hint; the plugin host is owned by `IO.REG.PLUGIN`.

## Children

_Leaf: none._
