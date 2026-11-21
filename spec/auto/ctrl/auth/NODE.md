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
updated: 2026-11-21
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

- **REQ-AUTO.CTRL.AUTH-001** When no token file exists, the app shall create one holding 32 random bytes from the OS CSPRNG, readable and writable only by the owning user (Unix mode 0600; Windows owner and protected DACL with the user's SID only, set when the file is created).
- **REQ-AUTO.CTRL.AUTH-002** If the token file grants any other principal access or is not a regular file (at start or at an `auth`), then the app shall refuse that authentication with `token_permissions` and atomically replace the file with a new owner-only token, so the old token is never accepted again.
- **REQ-AUTO.CTRL.AUTH-003** The server shall send a fresh 32-byte nonce and its instance ID in `hello`, and the client shall answer with its own 32-byte nonce and the proof HMAC-SHA256(token, label-c ‖ server nonce ‖ client nonce ‖ instance ID), where label-c is a fixed protocol string.
- **REQ-AUTO.CTRL.AUTH-004** After verifying the client proof in constant time against its own instance ID, the server shall answer with HMAC-SHA256(token, label-s ‖ client nonce ‖ server nonce ‖ instance ID), and the client shall close the connection if that proof is wrong.
- **REQ-AUTO.CTRL.AUTH-005** If any operation other than `hello` or `auth` arrives before authentication succeeds, or the proof is wrong, then the server shall reply `auth_failed` and close the connection.
- **REQ-AUTO.CTRL.AUTH-006** The token shall never appear in any message, log line or error text.

## Acceptance criteria

- **AC-1** Given a fresh user profile in the test harness, when the app starts, then the token file has 32 bytes and owner-only permissions (checked with the OS API, not by mode string only).
- **AC-2** Given the token file made group-readable (Unix) or given an Everyone read ACE (Windows), then a correct client gets `token_permissions`, the connection closes and the file holds a new owner-only token; after the permissions are narrowed again, the old token gets `auth_failed`.
- **AC-3** Given a correct client, when it authenticates, then both proofs verify and `commands.list` succeeds.
- **AC-4** (replay) Given a recorded `auth` message from a previous connection, when sent on a new connection, then the reply is `auth_failed` and the connection closes. Given two instances sharing the token file, a proof made for one is refused by the other (relay).
- **AC-5** (error path) Given `command.execute` sent before `auth`, then `auth_failed` and close.
- **AC-6** Given test vectors `fixtures/ctrl/auth-vectors.json` (token, nonces, instance ID, expected proofs), then the proof functions reproduce them exactly.
- **AC-7** Given a capture of all bytes on the connection and all logs, then the token bytes and their hex and base64 forms appear nowhere.

## Design

- **Registry command(s):** none.
- **Data structures:** `Token([u8; 32])` with zeroise-on-drop and no `Debug` output; `Nonce([u8; 32])`.
- **Algorithm:** HMAC-SHA256 (RFC 2104) with fixed ASCII labels `fid-ctrl-client-v1` and `fid-ctrl-server-v1`. The MAC input is label ‖ nonce ‖ nonce ‖ instance ID (UTF-8). The instance ID comes last and the other parts have fixed lengths, so the input is unambiguous. Equality is constant-time, and the server forgets its nonce after one `auth` attempt.
- **Instance binding:** before sending `auth`, the client compares `hello.instance_id` with the discovery file it connected from and closes on a mismatch. Binding the instance ID into both MACs makes a proof useless on another instance that shares the token file.
- **Wire shapes:** `hello` event `{kind:"hello", version, versions, instance_id, nonce}`, where `nonce` is present only before authentication. `auth` request params are exactly `{nonce, proof}`, the client's values. The success reply is the event `{kind:"auth", proof}`, the server's proof. Nonces and proofs are 64 lowercase hex digits. Failures are error events `auth_failed`, `token_permissions` or `token_io`, followed by close. Authentication must complete within the listener's first-request deadline.
- **Token file handling:** each new token is written to a temporary file that is owner-only from creation. Unix uses `O_EXCL` with mode 0600. Windows uses `CreateFileW(CREATE_NEW)` with a security descriptor giving owner = user SID and a protected DACL for that SID only; the applied descriptor is read back and checked before the secret is written, and the file is deleted if the check fails. The temporary file is then hard-linked to the token name when no file exists, or renamed over a refused file. Reads open without following links (Unix `O_NOFOLLOW|O_NONBLOCK`; Windows `FILE_FLAG_OPEN_REPARSE_POINT`, refusing reparse points), require a regular file and check owner and permissions on the handle. On Windows, BUILTIN\Administrators is accepted as owner only for a file the app did not create.
- **Refused files stay refused:** when a file is refused, its identity (Unix device and inode; Windows volume serial number and file index from `GetFileInformationByHandle`) is kept in memory. A file with a refused identity is refused (`token_io`) even if its permissions were narrowed back, until a replacement succeeds; each load retries the replacement and then reads the new token. A failed replacement is reported as `token_io`, never ignored. A replacement is skipped only when the current file is owner-only and has a different identity (a concurrent refusal already replaced it).
- **Token directory:** it must be private to the user. On Unix it is opened once with `O_DIRECTORY|O_NOFOLLOW` and checked through that descriptor (a directory, owned by the user, no group or other access). The token file's open, temporary create, link, rename and unlink are all relative to that descriptor (`openat`, `linkat`, `renameat`, `unlinkat`, through rustix). On Windows the directory must not be a reparse point, and its owner and every allow ACE that applies to it must be the user, LocalSystem or BUILTIN\Administrators, unless the ACE grants no write, delete or permission-changing right. It is checked at start and again before each replacement. A directory that fails is `ctrl_unsafe_runtime_dir`, before anything is written.
- **Stated limitations:** (1) On Windows, the operations after the directory check use paths, not a directory handle: the check and the rename are not atomic. Only the user, LocalSystem or Administrators can change the directory in between. (2) On Unix, only mode bits, owner and file type are inspected. POSIX ACLs (Linux) and extended ACLs (macOS, `chmod +a`) on the token file or its directory are not read, so an ACL that grants another user access is not detected. (3) The refused-identity set lives in memory, so a restart forgets it. At start, a refused file is still too open and is refused again; a file narrowed back while the app was down is accepted. (4) The discovery file's directory is still checked only on Unix (AUTO.CTRL.WIRE; it holds no secret).
- **Windows FFI:** the Win32 security calls are the named `unsafe` exception of D-048, crate `fid-ctrl-acl`, `mod ffi`.
- **Errors:** `token_permissions`, `auth_failed`, `token_io`.
- **Determinism:** N (random nonces); proof functions are D0 given inputs.
- **Performance budget:** handshake < 5 ms on loopback [unverified until measured].
- **Crate:** `fid-ctrl` (Windows ACL shim: `fid-ctrl-acl`, D-048)

## Test fixtures

`fixtures/ctrl/auth-vectors.json` computed once with an independent HMAC implementation and checked in (project-authored); permission tests per OS in CI.

## Build prompt

> In crate `fid-ctrl`, implement control-channel authentication. Token: 32 bytes from the OS CSPRNG in a token file created with owner-only permissions (Unix 0600 and owner check; Windows owner and protected DACL containing only the user SID, set at creation). If the file is wider or not a regular file, refuse that authentication with `token_permissions` and atomically replace it with a new token. Handshake: server `hello` carries a fresh 32-byte nonce and its instance ID, which the client checks against the discovery file. Client `auth` carries its 32-byte nonce and HMAC-SHA256(token, "fid-ctrl-client-v1" ‖ server_nonce ‖ client_nonce ‖ instance_id). The server verifies that in constant time and replies with HMAC-SHA256(token, "fid-ctrl-server-v1" ‖ client_nonce ‖ server_nonce ‖ instance_id), which the client verifies. Any other op before success, or a wrong proof → `auth_failed` and close. Nonces are single use. The token never appears in messages, logs or errors (zeroise on drop, no Debug). Acceptance: the seven cases in the spec, including replay of an old proof and fixed test vectors. Constraints: no panic, no `unsafe` beyond the OS ACL shim (named exception), deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; use a vetted HMAC and SHA-256 crate on the licence allowlist.

## Open questions

- None.

## Children

_Leaf: none._
