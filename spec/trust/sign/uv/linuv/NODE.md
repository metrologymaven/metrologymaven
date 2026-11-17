---
id: TRUST.SIGN.UV.LINUV
title: Linux token PIN and passphrase verification (D-011)
level: 4
parent: TRUST.SIGN.UV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.UV-001, REQ-TRUST.SIGN.UV-003, REQ-TRUST.SIGN.UV-004, REQ-TRUST.SIGN.UV-005]
covers: []
depends_on: [TRUST.SIGN.UV.GATE, TRUST.SIGN.KEYSTORE.LINUX]
side_effect: S1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.SIGN.UV.LINUV: Linux token PIN and passphrase verification (D-011)

## Purpose

Linux has no system user-verification API for signing keys, so D-011 option D applies: when the key is on a PKCS#11 token, the provider collects the token PIN for one operation (context-specific login) and reports `token_pin_touch` when the token enforces touch, else `token_pin`; for a passphrase-wrapped software key it collects the passphrase in the app's own secure prompt and hands it to the keystore to unwrap for that single operation (`passphrase`). Whether `passphrase` is acceptable is the caller's policy (Regulated excludes it).

## Scope

**In:**
- PIN prompt and `C_Login(CKU_CONTEXT_SPECIFIC)` per operation for keys with `CKA_ALWAYS_AUTHENTICATE`, or session login then logout around one sign otherwise
- Touch-policy detection where the token exposes it [unverified: vendor-specific]
- App-owned passphrase prompt (GUI dialog, or no-echo TTY for an upgraded CLI session)
- Zeroising PIN and passphrase buffers

**Out:**
- The gate and policy (`TRUST.SIGN.UV.GATE`)
- Key storage, wrapping and unwrapping (`TRUST.SIGN.KEYSTORE.LINUX`)
- Windows and macOS (`TRUST.SIGN.UV.OSUV`)

## Requirements

- **REQ-TRUST.SIGN.UV.LINUV-001** For a token key, the provider shall prompt for the PIN on every operation, log in for that one signature only, and log out (or end the context-specific login) before returning.
- **REQ-TRUST.SIGN.UV.LINUV-002** The provider shall report `token_pin_touch` only when the token reports a touch requirement for the key, and `token_pin` otherwise.
- **REQ-TRUST.SIGN.UV.LINUV-003** For a wrapped software key, the provider shall prompt for the passphrase on every operation, pass it to the keystore for a single unwrap, and report `passphrase`.
- **REQ-TRUST.SIGN.UV.LINUV-004** PIN and passphrase buffers shall be zeroised before the call returns, and shall never be logged, journaled or cached.
- **REQ-TRUST.SIGN.UV.LINUV-005** If the PIN is wrong, the token is locked, or the passphrase fails to unwrap, then the provider shall return `failed` with the token or keystore reason, and after 3 consecutive wrong passphrases in one session it shall refuse further attempts for 60 s [unverified: rate values].
- **REQ-TRUST.SIGN.UV.LINUV-006** On an upgraded CLI session the prompt shall read from the controlling terminal with echo disabled and shall refuse with `unavailable` when no terminal is attached.

## Acceptance criteria

- **AC-1** Given SoftHSM2 with a P-256 key and a scripted PIN source, when two signatures are requested through `UvGate`, then two logins occur, both signatures verify and the method is `token_pin`.
- **AC-2** Given a wrapped software key and a scripted passphrase source, then the method is `passphrase`; given a policy excluding `passphrase` (Regulated), the gate returns `sign.uv_method_not_allowed` before unwrap.
- **AC-3** (error path) Given a wrong PIN, then `failed` and no signature; given three wrong passphrases, the fourth attempt within 60 s returns `failed(rate_limited)` without prompting.
- **AC-4** Given a CLI session with stdin not a TTY, then `unavailable`.
- **AC-5** (manual, hardware) Given a hardware token with a touch policy, then the method is `token_pin_touch`; recorded in `qual/manual/uv-linux-token.md`.

## Design

- **Registry command(s):** none.
- **Data structures:** `LinuxUvProvider { prompt: Box<dyn SecretPrompt> }`; `Secret(Zeroizing<Vec<u8>>)`.
- **Algorithm:** inspect the key's backend kind; token: prompt PIN, login, issue token bound to the session, sign, logout; software: prompt passphrase, issue token carrying it to the keystore's unwrap-and-sign.
- **Errors:** `UvError::{Failed(reason), Cancelled, Unavailable}`.
- **Determinism:** N.
- **Performance budget:** none (human prompt; Argon2id cost is the keystore's).
- **Crate:** `fid-uv`, module `linux` (`cfg(target_os = "linux")`).

## Test fixtures

SoftHSM2 config from `TRUST.SIGN.KEYSTORE.CMDS` fixtures; scripted `SecretPrompt` doubles; manual hardware record for touch.

## Build prompt

> In crate `fid-uv`, add `cfg(target_os = "linux")` module `linux` implementing `UvProvider` per D-011 option D. Token keys: prompt for the PIN each operation (GUI dialog, or no-echo TTY on an upgraded CLI session; `unavailable` without a TTY), log in for one signature (context-specific login when `CKA_ALWAYS_AUTHENTICATE`), log out; method `token_pin_touch` only if the token reports a touch requirement, else `token_pin`. Wrapped software keys: prompt for the passphrase each operation and hand it to `fid-keystore::linux` for a single unwrap-and-sign; method `passphrase`. Zeroise secrets; never log, journal or cache them. Wrong PIN or passphrase → `Failed(reason)`; 3 wrong passphrases per session → 60 s refusal. Prompts behind a `SecretPrompt` trait for tests. Tests: AC-1..AC-4 with SoftHSM2; AC-5 manual. Constraints: D-011, CONSTITUTION §8, REQ-TRUST.SIGN-004/005.

## Open questions

- Touch detection is vendor-specific [unverified]; when unknown, the method is reported as `token_pin`, never overstated.

## Children

_Leaf: none._
