---
id: AUTO.PY.PROTO
title: Script host protocol for Python and other languages
level: 3
parent: AUTO.PY
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-004, REQ-AUTO.PY-008, REQ-AUTO.PY-010]
covers: [PCD.PRG.06]
depends_on: [AUTO.SURF.ENV, AUTO.SURF.JOBS, AUTO.SURF.ROOTS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PY.PROTO: Script host protocol for Python and other languages

## Purpose

The message protocol between the script host and a script process, published as a versioned spec so that a client written in any language can run as a script with exactly the same principal, root and class rules as Python. The host, not the script, owns the principal; the script can only send command invocations, dialog requests, log lines and progress.

## Scope

**In:**
- Protocol spec document and JSON Schemas (versioned)
- Framing over the child process's stdin and stdout (JSON lines), stderr for free-form logs
- Messages: `hello` (versions, manifest digest), `invoke`, `accepted`, `result` (envelope), `progress`, `cancel`, `dialog.show`, `dialog.answer`, `log`, `bye`
- Python reference client inside the `fiducial` package (`fiducial._host`)
- Conformance transcripts usable to test any client

**Out:**
- Process management (`AUTO.PY.HOST`)
- Dialog descriptors (`AUTO.PROG.OPER`) and dialog rendering (`VIZ`)

## Requirements

- **REQ-AUTO.PY.PROTO-001** The protocol shall offer no message that reads or writes project state other than `invoke`, which names a registry command, its major, parameters and an idempotency key.
- **REQ-AUTO.PY.PROTO-002** The protocol shall carry no principal field from the script; the host shall attach the run's principal to every invocation.
- **REQ-AUTO.PY.PROTO-003** Every file parameter in an `invoke` shall be a root handle, and the host shall reply `raw_path_refused` to any raw path.
- **REQ-AUTO.PY.PROTO-004** The protocol spec shall be published with JSON Schemas for every message and a version number negotiated in `hello`.
- **REQ-AUTO.PY.PROTO-005** The project shall publish conformance transcripts that a client in any language can be tested against, and the Python reference client shall pass them.
- **REQ-AUTO.PY.PROTO-006** Every message shall be at most 4 MiB [unverified], and an oversized or malformed message shall end the run with `protocol_violation` without affecting the app.

## Acceptance criteria

- **AC-1** Given conformance transcripts `fixtures/script-proto/*.jsonl`, when the Python reference client is driven by the transcript runner, then every transcript passes.
- **AC-2** Given a minimal client written in a second language (a 100-line Rust test client in the repo), when run against the same transcripts, then they pass, showing the spec is sufficient without Python.
- **AC-3** (error path) Given an `invoke` with a parameter `principal: "human"`, then the host ignores it and the fake bus receives the run's principal.
- **AC-4** (error path) Given an `invoke` with a raw path, then the reply is `raw_path_refused`.
- **AC-5** (error path) Given a 5 MiB line, then the run ends with `protocol_violation` and the app test harness keeps running.
- **AC-6** Given each message type, then it validates against its JSON Schema in `schema/script-proto-v1/`.

## Design

- **Registry command(s):** none.
- **Data structures:** message enum with `v`, `id`, `type`, body; `invoke { command, major, params, idempotency_key }`.
- **Algorithm:** JSON lines on stdin and stdout of the child; bounded reader; host-side validation before any bus call; the Python client is synchronous by default with an async variant.
- **Errors:** `protocol_violation`, `version_unsupported`, `raw_path_refused`, plus envelope errors.
- **Determinism:** D0 for message encoding.
- **Performance budget:** < 1 ms protocol overhead per invocation [unverified until measured].
- **Crate:** `fid-script-host` (host side and transcript runner); `fiducial._host` (Python client).

## Test fixtures

Conformance transcripts under `fixtures/script-proto/` (project-authored, CC-BY-4.0); fake bus.

## Build prompt

> Specify and implement the script host protocol. Host side in crate `fid-script-host`; Python client as `fiducial._host`. Framing: JSON lines on the child's stdin/stdout, stderr for logs, 4 MiB per message, bounded reads. Messages: hello (version negotiation, manifest digest), invoke (command, major, params, idempotency key), accepted, result (shared envelope), progress, cancel, dialog.show, dialog.answer, log, bye. The script never sends a principal; the host attaches the run's principal and ignores principal-like fields. Raw paths → `raw_path_refused`. Oversized or malformed messages end the run with `protocol_violation` without affecting the app. Publish the spec as Markdown plus JSON Schemas in `schema/script-proto-v1/`, and conformance transcripts in `fixtures/script-proto/` with a transcript runner. Acceptance: Python client and a minimal second-language test client both pass all transcripts; the error cases in the spec. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; all input hostile.

## Open questions

- None.

## Children

_Leaf: none._
