---
id: HAL.HOST.SUPER
title: Driver host launch, allowlist check and supervision
level: 3
parent: HAL.HOST
kind: task
status: leaf
phase: P2
size: M
owners: []
claimed_by:
satisfies: [REQ-HAL.HOST-002, REQ-HAL.HOST-003]
covers: []
depends_on: [HAL.HOST.PKG, HAL.HOST.PROTO, HAL.ACQ.SESS]
side_effect: D1
determinism: N
cli: []
mcp: []
journal: [device.event]
updated: 2026-11-16
---

# HAL.HOST.SUPER: Driver host launch, allowlist check and supervision

## Purpose

The runtime component that starts a local Tier-2 driver host (or connects to a paired remote one), verifies it is exactly the installed, allowlisted binary, watches its heartbeats, and handles crashes without taking the engine down. It turns REQ-HAL-006 ("a failing driver shall not crash the engine") into concrete behaviour.

## Scope

**In:**
- Launch of a local host process as a low-privilege child, with its arguments and channel endpoints
- Pre-launch check: package signature valid and binary digest on the installed allowlist
- Heartbeat monitoring and host-loss detection (three missed heartbeats, process exit, or crash)
- On loss: sessions `INTERRUPTED`, stamped batches stay valid, a `device.event` is journaled
- Restart with exponential back-off and a restart budget
- For remote hosts: connection-time digest check of the reported host binary against the allowlist

**Out:**
- Package installation and the allowlist store (owned by `HAL.HOST.PKG`)
- Signature verification primitives (owned by `ECO` and `TRUST`; called here)
- The wire protocol (owned by `HAL.HOST.PROTO`)
- Loading Tier-3 components (owned by `HAL.HOST.WASM`)

## Requirements

- **REQ-HAL.HOST.SUPER-001** Before launching a local host, the supervisor shall verify the package signature and that the SHA-256 digest of the host binary is on the installed allowlist, and shall refuse to launch with `driver_not_allowlisted` on any mismatch.
- **REQ-HAL.HOST.SUPER-002** When a remote host connects, the supervisor shall compare its reported binary digest with the allowlist and refuse the connection with `driver_not_allowlisted` on mismatch.
- **REQ-HAL.HOST.SUPER-003** If a host misses three consecutive heartbeats, exits or crashes, then within 4 s of the last heartbeat the supervisor shall mark every session served by it `INTERRUPTED`, journal a `device.event` with reason, and keep already-stamped batches valid.
- **REQ-HAL.HOST.SUPER-004** After host loss, the supervisor shall restart a local host with exponential back-off starting at 1 s and capped at 60 s, and shall stop after 5 failed restarts within 10 minutes and mark the driver `FAULT`.
- **REQ-HAL.HOST.SUPER-005** A host loss shall never terminate the engine process or affect sessions served by other hosts.
- **REQ-HAL.HOST.SUPER-006** A restarted host shall not resume a session automatically; resumption shall require an explicit resume command from `HAL.ACQ.SESS`.

## Acceptance criteria

- **AC-1** Given `fixtures/hal/host/super/crashy_host` configured to abort after 50 batches, when a session streams, then the session becomes `INTERRUPTED`, 50 batches remain valid in the journal, one `device.event` with reason `HOST_EXITED` is journaled, and a parallel session on `fixtures/hal/host/super/steady_host` keeps streaming.
- **AC-2** Given `crashy_host` configured to stop heartbeats but keep running, when 3 s pass, then the session is `INTERRUPTED` within 4 s of the last heartbeat with reason `HEARTBEAT_LOST`.
- **AC-3** (error path) Given `steady_host` with one byte changed after install, when a session opens, then launch is refused with `driver_not_allowlisted` and nothing is launched.
- **AC-4** Given `crashy_host` configured to crash at start, when supervision runs, then restart attempts occur at about 1, 2, 4, 8 and 16 s and the driver is then marked `FAULT` with no sixth attempt.
- **AC-5** Given a host that restarts successfully, when it reconnects, then the session stays `INTERRUPTED` until `device.resume_session` is issued.

## Design

- **Registry commands:** none of its own; launch happens inside `device.open_session@1` (D1). Events: `device.event` with reasons `HOST_EXITED`, `HOST_CRASHED`, `HEARTBEAT_LOST`, `DRIVER_FAULTED`.
- **Data structures:** `HostHandle { pid, package_id, digest, state }`, `RestartPolicy { base: 1 s, cap: 60 s, budget: 5 per 10 min }`.
- **Algorithm:** digest the binary immediately before launch (avoid check-then-swap by launching from a read-only, digest-verified copy); monotonic-clock heartbeat timer; back-off doubling.
- **Errors:** `driver_not_allowlisted`, `driver_signature_invalid`, `host_launch_failed`.
- **Determinism:** N (process and timing behaviour).
- **Performance budget:** detection within 4 s of last heartbeat (REQ-003).
- **Crate:** `fid-hal-runtime` (module `supervise`).

## Test fixtures

`fixtures/hal/host/super/crashy_host` and `steady_host`: tiny project-written host binaries speaking the host protocol, with failure modes selected by command-line flags (Apache-2.0 test code), signed with a test key in a test allowlist.

## Build prompt

> In crate `fid-hal-runtime`, module `supervise`, implement the Tier-2 host supervisor. Before launch: verify package signature (through the ECO/TRUST verification API) and the SHA-256 of the host binary against the installed allowlist, launching from a read-only verified copy; refuse with `driver_not_allowlisted` / `driver_signature_invalid`. For remote hosts, check the reported digest at connect. Monitor heartbeats (1 s interval; 3 missed = loss), process exit and crash; on loss mark served sessions `INTERRUPTED` within 4 s, journal `device.event` with reason, keep stamped batches valid, never stop the engine. Restart local hosts with back-off 1 s doubling to 60 s, at most 5 failures per 10 min, then `FAULT`. Never auto-resume sessions. No panic, unwrap, expect or unsafe (CONSTITUTION §8). Tests: AC-1..AC-5 with fixture hosts in `fixtures/hal/host/super/`.

## Open questions

- Running local hosts under a separate low-privilege OS account needs per-OS setup that `PLAT` owns; this leaf launches with reduced privileges where the OS allows and records what it could do.

## Children

_Leaf: none._
