---
id: CORE.BUS.POL.EGRESS
title: Egress primitive and network guard
level: 4
parent: CORE.BUS.POL
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
satisfies: [REQ-CORE.BUS.POL-007, REQ-CORE.BUS.POL-008]
covers: []
depends_on: [CORE.REG.DEF, CORE.BUS.PRIN, CORE.BUS.PIPE.ERR, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.FS, CORE.STO.CAS, CORE.STO.SET]
side_effect: X1
determinism: N
cli: [fid egress send]
mcp: [egress_send]
journal: [egress.send@1]
updated: 2026-11-17
---

# CORE.BUS.POL.EGRESS: Egress primitive and network guard

## Purpose

The only way data leaves the machine (CONSTITUTION §3.1, D-031). `egress.send@1` takes an artifact already staged in the content-addressed store by another domain and a destination declared by an administrator, and sends it only with a human approval recorded for that invocation. The network guard makes sure the bus and every handler can open sockets only while an X1, I1 or D1 command is executing. Per D-023, device control and readings to a configured remote driver host stay D1; project data sent to any other machine goes through this primitive.

## Scope

**In:** `egress.send@1` (X1); the destination model (`network_root` for a network share, `https` for a configured endpoint); the per-invocation human approval; the `NetPermit` token and its issue rule; the socket-denial test harness.
**Out:** staging artifacts (each domain, for example `VIZ.CAPTURE`, `PLAT.DOCTOR`, `RPT`); configuring destinations (an A0 setting in `CORE.STO.SET`); remote driver-host protocol (`HAL`, `PLAT.XHOST`); pending-request filing for agents (`CORE.BUS.POL.PROJ`); local export roots (`CORE.BUS.POL.FS`, W1).

## Requirements

- **REQ-CORE.BUS.POL.EGRESS-001** When `egress.send@1` runs with a staged artifact digest and a destination ID, the bus shall send exactly the artifact bytes whose SHA-256 matches the digest to that destination and record the digest, size, destination ID and outcome in the entry.
- **REQ-CORE.BUS.POL.EGRESS-002** If `egress.send@1` is invoked without an approval recorded for that invocation, by a `human` principal confirming the artifact digest and the destination, then the bus shall not send anything; for `agent` and `script` principals the call shall become a pending request.
- **REQ-CORE.BUS.POL.EGRESS-003** If the destination ID is not configured, or the artifact is not in the store, or its digest does not verify, then the bus shall return `policy` (reason `bad_destination`) or `validation` (reason `bad_artifact`) and send nothing.
- **REQ-CORE.BUS.POL.EGRESS-004** The bus shall issue a `NetPermit` only to the handler of an executing X1, I1 or D1 command, for that execution only, and every socket-opening API in product crates shall require one.
- **REQ-CORE.BUS.POL.EGRESS-005** While no X1, I1 or D1 command is executing, the process shall open no network connection, as shown by a test harness that runs every registered non-X1/I1/D1 test command under an OS sandbox that denies sockets.
- **REQ-CORE.BUS.POL.EGRESS-006** If a send fails or is cancelled, then the entry shall record the failure outcome, and the bus shall not retry automatically.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/bus/pol/egress/artifact.bin` staged in a test store and a test `https` destination served by a local test server, when a GUI human runs `egress.send@1` with the approval dialog confirmed, then the server receives bytes whose SHA-256 equals the digest, and the entry records digest, size, destination and `sent`.
- **AC-2** (error path) Given an MCP agent invoking `egress_send`, then a pending request is filed and the test server receives nothing; when a human approves it, then the bytes are sent once.
- **AC-3** (error path) Given an unknown destination ID or a tampered artifact, then `bad_destination` or `bad_artifact`, and nothing is sent.
- **AC-4** Given the harness `net_guard_suite` on Linux (network namespace with no interfaces) [unverified for Windows and macOS mechanisms], when every registered R0, R1, W1, W2 and W3 test command runs, then no socket is opened and all pass.
- **AC-5** (error path) Given a test handler of a W2 command that tries to open a socket without a `NetPermit`, then it fails to compile (the API requires the permit).

## Design

- **Registry command(s):** `egress.send@1`, X1, determinism N; params `{ artifact: Digest, destination: DestinationId, note?: Untrusted<String> (≤500) }`; result `{ outcome: sent|failed|cancelled, bytes, remote_receipt?: Untrusted<String> }`; projection `request_only`. CLI `fid egress send`; MCP `egress_send` files a request.
- **Data structures:** `Destination { id, kind: NetworkRoot { path } | Https { url, method: PUT|POST } }` from A0 settings; `EgressApproval { approver, artifact, destination, at }` carried in the invocation; `NetPermit` (non-cloneable, lifetime-bound to one execution).
- **Algorithm:** check approval matches digest and destination; load and verify the blob; stream it to the destination under a `NetPermit`; record the outcome. Socket APIs are wrapped in a `fid-net` facade that takes `&NetPermit`; `CORE.BUS.LINT`-style checks deny direct socket crates outside the facade.
- **Errors:** `policy` (reasons `bad_destination`, `no_approval`), `validation` (`bad_artifact`), `cancelled`.
- **Determinism:** N (network outcome is observed and recorded).
- **Performance budget:** throughput limited by the destination; overhead < 5 ms per call [unverified until measured].
- **Crate:** `fid-bus` (module `policy::egress`) and facade crate `fid-net`.

## Test fixtures

`fixtures/core/bus/pol/egress/artifact.bin` (random bytes from a fixed seed), a local HTTPS test server with a self-signed certificate created at test time; CC-BY-4.0.

## Build prompt

> In crate `fid-bus`, add module `policy::egress` and register `egress.send@1` (X1, determinism N, projection request_only; CLI `fid egress send`, MCP `egress_send`). Params: staged artifact digest, destination ID from A0 settings (`network_root` path or `https` URL with PUT/POST), optional untrusted note. Execute only with an `EgressApproval` from a human principal matching digest and destination (`no_approval` otherwise; agent and script calls become pending requests through `policy::projection`). Verify the blob digest (`bad_artifact`), check the destination (`bad_destination`), send under a `NetPermit`, record digest, size, destination and outcome; never retry automatically. Create facade crate `fid-net` whose socket-opening functions require `&NetPermit`; the pipeline issues a permit only to executing X1, I1 or D1 handlers. Add `net_guard_suite` running all non-X1/I1/D1 test commands in a socket-denying sandbox. Tests: AC-1..AC-5. Constraints: CONSTITUTION C7 (local first), §3 and §3.1 (D-031 egress primitive, D-023 remote hosts), §8.

## Open questions

- **Revise for D-035 (accepted 2026-11-17):** `egress.send@1` returns a bounded, size-capped response body (fetch = empty payload; response is untrusted data), and grant a scoped read-only socket permit for the R0 driver-host handshake (D-023).
- A lint that bans socket crates outside `fid-net` fits `CORE.BUS.LINT`, which is a leaf owned elsewhere; this leaf relies on the type requirement until that check is extended.
- Socket-denial mechanisms on Windows and macOS for AC-4 need a platform review [unverified].
- **Build status (first build PR, not yet `built_in`):** built in `fid-bus` (`policy::egress`) and `fid-net`: the destination model and its A0 setting, strict `https` URL and ID grammar, `egress.send@1` definition, authorise (agent and script calls file requests through `policy::projection`, humans need an `EgressApproval` from the approval dialog bound to the digest, destination ID and destination configuration digest), artifact verification, send under a `NetPermit` with outcome recording and no retry, the `network_root` transport, and the permit rule (only `fid-bus` can issue a permit; `fid-net` socket functions require one; AC-2, AC-3, AC-5). Remaining: an `https` transport (TLS stack, trust roots, no-redirect client) and the AC-1 local HTTPS test server; `net_guard_suite` (AC-4), which needs the registered command set and the pipeline (`CORE.BUS.PIPE.STAGE`); STAGE must carry the `EgressApproval` in the invocation and call `issue_permit` for X1, I1 and D1 handlers only; artifacts larger than one store blob (chunked objects) are refused with `bad_artifact` until a digest rule for them is set.
- Redirects and DNS are not covered by this spec. The transport contract treats any redirect as `failed` and allows connecting only to the configured host and port; DNS rebinding of a configured host name is open.

## Children

_Leaf: none._
