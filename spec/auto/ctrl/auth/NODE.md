---
id: AUTO.CTRL.AUTH
title: Token file and challenge-response authentication
level: 3
parent: AUTO.CTRL
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
built_in: [fid-ctrl, fid-ctrl-acl]
built_pr: 410
satisfies: [REQ-AUTO.CTRL-002, REQ-AUTO.CTRL-003, REQ-AUTO.CTRL-004]
covers: []
depends_on: [AUTO.CTRL.WIRE]
side_effect: R0
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CTRL.AUTH: Token file and challenge-response authentication

## Purpose

Proves that a client runs as the same OS user as the app without ever sending the secret: the app keeps a random token in a file only that user can read, and each connection authenticates with keyed-MAC proofs over fresh nonces from both sides, so a captured exchange cannot be replayed.

## Scope

**In:**
- Token file creation (256-bit random), permission setting and permission verification
- Handshake: server nonce in `hello`, client `auth` with client nonce and proof, server proof back (mutual)
- Rejection of any operation before authentication; close on first failure
- Constant-time comparison; per-connection single-use nonces

**Out:**
- Token directory per OS (`PLAT`)
- What an authenticated client may do (`AUTO.CTRL.EXEC`)

## Requirements

- **REQ-AUTO.CTRL.AUTH-001** When no token file exists, the app shall create one holding 32 random bytes from the OS CSPRNG, readable and writable only by the owning user (Unix mode 0600; Windows protected DACL with the owner's SID only).
- **REQ-AUTO.CTRL.AUTH-002** If the token file's permissions grant any other principal access, then the app shall refuse every authentication with `token_permissions` until the file is recreated.
- **REQ-AUTO.CTRL.AUTH-003** The server shall send a fresh 32-byte nonce in `hello`, and the client shall answer with its own 32-byte nonce and the proof HMAC-SHA256(token, label-c ‖ server nonce ‖ client nonce), where label-c is a fixed protocol string.
- **REQ-AUTO.CTRL.AUTH-004** After verifying the client proof in constant time, the server shall answer with HMAC-SHA256(token, label-s ‖ client nonce ‖ server nonce), and the client shall close the connection if that proof is wrong.
- **REQ-AUTO.CTRL.AUTH-005** If any operation other than `hello` or `auth` arrives before authentication succeeds, or the proof is wrong, then the server shall reply `auth_failed` and close the connection.
- **REQ-AUTO.CTRL.AUTH-006** The token shall never appear in any message, log line or error text.

## Acceptance criteria

- **AC-1** Given a fresh user profile in the test harness, when the app starts, then the token file has 32 bytes and owner-only permissions (checked with the OS API, not by mode string only).
- **AC-2** Given the token file made group-readable (Unix) or given an Everyone read ACE (Windows), then a correct client gets `token_permissions` and the connection closes.
- **AC-3** Given a correct client, when it authenticates, then both proofs verify and `commands.list` succeeds.
- **AC-4** (replay) Given a recorded `auth` message from a previous connection, when sent on a new connection, then the reply is `auth_failed` and the connection closes.
- **AC-5** (error path) Given `command.execute` sent before `auth`, then `auth_failed` and close.
- **AC-6** Given test vectors `fixtures/ctrl/auth-vectors.json` (token, nonces, expected proofs), then the proof functions reproduce them exactly.
- **AC-7** Given a capture of all bytes on the connection and all logs, then the token bytes and their hex and base64 forms appear nowhere.

## Design

- **Registry command(s):** none.
- **Data structures:** `Token([u8; 32])` with zeroise-on-drop and no `Debug` output; `Nonce([u8; 32])`.
- **Algorithm:** HMAC-SHA256 (RFC 2104) with fixed ASCII labels `fid-ctrl-client-v1` and `fid-ctrl-server-v1`; constant-time equality; the server forgets its nonce after one `auth` attempt.
- **Errors:** `token_permissions`, `auth_failed`, `token_io`.
- **Determinism:** N (random nonces); proof functions are D0 given inputs.
- **Performance budget:** handshake < 5 ms on loopback [unverified until measured].
- **Crate:** `fid-ctrl`

## Test fixtures

`fixtures/ctrl/auth-vectors.json` computed once with an independent HMAC implementation and checked in (project-authored); permission tests per OS in CI.

## Build prompt

> In crate `fid-ctrl`, implement control-channel authentication. Token: 32 bytes from the OS CSPRNG in a token file created with owner-only permissions (Unix 0600 and owner check; Windows protected DACL containing only the owner SID); if permissions are wider, refuse all authentication with `token_permissions`. Handshake: server `hello` carries a fresh 32-byte nonce; client `auth` carries its 32-byte nonce and HMAC-SHA256(token, "fid-ctrl-client-v1" ‖ server_nonce ‖ client_nonce); server verifies in constant time and replies with HMAC-SHA256(token, "fid-ctrl-server-v1" ‖ client_nonce ‖ server_nonce); the client verifies that. Any other op before success, or a wrong proof → `auth_failed` and close. Nonces are single use. The token never appears in messages, logs or errors (zeroise on drop, no Debug). Acceptance: the seven cases in the spec, including replay of an old proof and fixed test vectors. Constraints: no panic, no `unsafe` beyond the OS ACL shim (named exception), deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; use a vetted HMAC and SHA-256 crate on the licence allowlist.

## Open questions

- None.

## Children

_Leaf: none._
