---
id: IO.CAD.WORKER.BIN
title: Sandboxed CAD worker executable
level: 4
parent: IO.CAD.WORKER
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.WORKER-003, REQ-IO.CAD.WORKER-005]
covers: []
depends_on: [IO.CAD.WORKER.PROTO, IO.CAD.STEP.BREP, IO.CAD.STEP.ASM, IO.CAD.PMI.READ, IO.CAD.IGES.ENT]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.CAD.WORKER.BIN: Sandboxed CAD worker executable

## Purpose

The `fid-cad-worker` executable: it drops its own privileges as soon as it starts (no file-system writes, no network), answers the handshake with its reader name and version, and runs the first-party pure-Rust STEP and IGES readers on bytes it receives over the protocol, streaming typed records back. Under D-024 (option C) these first-party readers are Rust; OCCT or other healers are optional plugins hosted by `IO.REG.PLUGIN`, never this executable.

## Scope

**In:**
- Process entry: read nothing from argv or environment except a protocol version check
- Self-restriction before reading the first request: Windows (restricted token or AppContainer [unverified: which one is reachable without `unsafe`]), Linux (seccomp filter plus Landlock where the kernel supports it [unverified: kernel availability]), macOS (sandbox profile [unverified: API status])
- Handshake answer: reader name `fid-cad`, crate version, protocol version, sandbox level (`full|partial`)
- Dispatch of `LoadStep` to the STEP reader (`IO.CAD.STEP.BREP`, `IO.CAD.STEP.ASM`) and, when the request sets `pmi`, the PMI graph reader (`IO.CAD.PMI.READ`); `LoadIges` goes to the IGES entity mapper (`IO.CAD.IGES.ENT`), streaming records in source order

**Out:**
- Caps, kill and failure mapping (`IO.CAD.WORKER.HOST`)
- Reader logic (`IO.CAD.STEP.*`, `IO.CAD.IGES.*`)

## Requirements

- **REQ-IO.CAD.WORKER.BIN-001** Before reading its first request, the worker shall restrict itself so that opening a file for writing and opening a network socket both fail.
- **REQ-IO.CAD.WORKER.BIN-002** If the platform cannot apply the full restriction, then the worker shall report `sandbox: partial` in its handshake and the host-side import provenance shall record it.
- **REQ-IO.CAD.WORKER.BIN-003** The handshake answer shall carry the reader name, the reader version and the protocol version.
- **REQ-IO.CAD.WORKER.BIN-004** For the same request bytes, the worker shall emit an identical record sequence.

## Acceptance criteria

- **AC-1** Given the worker started in self-test mode on each Tier 1 OS, when it tries to create `probe.tmp` in its working directory and to connect to 127.0.0.1:9, then both fail and the self-test reports `sandbox: full` (or `partial` with the missing mechanism on platforms listed in Open questions).
- **AC-2** Given `fixtures/io/cad/step/brep/cube_ap242.stp` sent as `LoadStep`, when the worker runs twice, then the two record streams are byte-identical.
- **AC-3** Given a handshake, then `HelloAck` carries `reader: fid-cad`, the crate version and protocol major 1.
- **AC-4** (error path) Given a `LoadIges` request whose bytes are an STL file, then the worker answers `Failed{code: malformed}` and exits 0.

## Design

- **Registry commands:** none (executable).
- **Algorithm:** restrict → handshake → loop { read request → run reader with a `Budget` built from request options → stream `Records` in batches of ≤ 4,096 → `Report` } → exit on `Shutdown` or EOF.
- **Errors:** reader errors are sent as `Failed{code}` using `IO.REG.LIMITS` codes; the worker never panics (panic = abort is set, so any bug surfaces as `reader_crashed` on the host).
- **Determinism:** D0 for the record stream.
- **Performance budget:** startup to `HelloAck` < 50 ms [unverified until measured].
- **Crate:** `fid-cad-worker` (binary).

## Test fixtures

Uses STEP and IGES fixtures owned by `IO.CAD.STEP.BREP` and `IO.CAD.IGES.ENT`; a self-test mode built into the binary.

## Build prompt

> Create binary crate `fid-cad-worker`. On start, restrict the process (Windows restricted token or AppContainer, Linux seccomp + Landlock, macOS sandbox profile; use safe wrapper crates on the D-001 allowlist, `unsafe` only with a named exception), then answer `Hello` from `fid-cad-proto` with `reader: fid-cad`, crate version, protocol version and `sandbox: full|partial`. Dispatch `LoadStep` to the `fid-io-step` reader (plus `fid-io-pmi` `read_pmi` when `pmi` is set) and `LoadIges` to the `fid-io-iges` reader; stream records in source order, batches ≤ 4,096; reader errors become `Failed{code}`. Read nothing from argv or env except a `--self-test` flag that tries a file write and a socket connect and reports the result. Profile `panic = "abort"`. CONSTITUTION §8: no unwrap, no panic paths. Tests AC-1..AC-4.

## Open questions

- Exact sandbox mechanisms per OS and whether each is reachable without `unsafe` [unverified]; the PLAT support matrix should list the sandbox level per platform.

## Children

_Leaf: none._
