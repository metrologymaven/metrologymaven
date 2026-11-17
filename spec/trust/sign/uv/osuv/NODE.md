---
id: TRUST.SIGN.UV.OSUV
title: Windows and macOS per-operation user verification
level: 4
parent: TRUST.SIGN.UV
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-TRUST.SIGN.UV-001, REQ-TRUST.SIGN.UV-003, REQ-TRUST.SIGN.UV-004]
covers: []
depends_on: [TRUST.SIGN.UV.GATE, TRUST.SIGN.KEYSTORE.WIN, TRUST.SIGN.KEYSTORE.MAC]
side_effect: S1
determinism: N
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.SIGN.UV.OSUV: Windows and macOS per-operation user verification

## Purpose

The `UvProvider` implementations for the two platforms whose OS offers user verification. On macOS the key's access control already demands user presence; this provider creates a fresh `LAContext` with no reuse window for each signature and hands it to the key handle (`macos_local_auth`). On Windows it runs a Windows Hello consent verification immediately before the CNG sign and issues the single-use token only on success (`windows_hello`).

## Scope

**In:**
- macOS provider: fresh `LAContext` per operation, reason string, reuse duration 0
- Windows provider: Windows Hello `UserConsentVerifier` check per operation, availability detection
- Mapping OS results to `UvError` (failed, cancelled, unavailable)

**Out:**
- The gate, policy and timeout (`TRUST.SIGN.UV.GATE`)
- Key creation and access-control flags (`TRUST.SIGN.KEYSTORE.WIN`, `.MAC`)
- Linux (`TRUST.SIGN.UV.LINUV`)

## Requirements

- **REQ-TRUST.SIGN.UV.OSUV-001** On macOS, the provider shall create a new `LAContext` for each operation with a reuse duration of zero and pass it to the sign call, so the Keychain demands user presence for that one operation.
- **REQ-TRUST.SIGN.UV.OSUV-002** On Windows, the provider shall run a Windows Hello consent verification for each operation and shall issue the single-use `UvToken` only when it returns verified.
- **REQ-TRUST.SIGN.UV.OSUV-003** The prompt text shall name the action and the key label, and shall be fixed text plus the key label, containing no package content.
- **REQ-TRUST.SIGN.UV.OSUV-004** If the OS reports cancellation, failure, or that no verification method is configured, then the provider shall return `cancelled`, `failed` or `unavailable` respectively.
- **REQ-TRUST.SIGN.UV.OSUV-005** The provider shall report `macos_local_auth` on macOS and `windows_hello` on Windows.

## Acceptance criteria

- **AC-1** (manual, hardware) Given a macOS machine with Touch ID, when two signatures are requested back to back through `UvGate`, then two prompts appear and both signatures verify with `fid-sign-core`; recorded in `qual/manual/uv-macos.md`.
- **AC-2** (manual, hardware) Given a Windows machine with Windows Hello, then the same holds with two Hello prompts; recorded in `qual/manual/uv-windows.md`.
- **AC-3** Given the providers with the OS API replaced by a test double returning cancel, fail and not-configured, then `cancelled`, `failed` and `unavailable` are returned and no token is issued.
- **AC-4** Given a CI build on both OSes, then the modules compile and AC-3 passes.

## Design

- **Registry command(s):** none.
- **Data structures:** `MacUvProvider`, `WinUvProvider`, both `impl UvProvider`.
- **Algorithm:** macOS: `LAContext()` with `touchIDAuthenticationAllowableReuseDuration = 0`, attached to the sign query; Windows: `UserConsentVerifier::RequestVerificationAsync(message)` then issue token; the CNG key itself is not cryptographically bound to the Hello check [unverified: whether a CNG UI policy can bind it].
- **Errors:** `UvError::{Failed, Cancelled, Unavailable}`.
- **Determinism:** N.
- **Performance budget:** none (human prompt).
- **Crate:** `fid-uv`, modules `mac` and `win` (`cfg`-gated; FFI only through maintained wrapper crates or a named CONSTITUTION §8 exception).

## Test fixtures

OS-API test doubles in code; manual hardware runs recorded as QUAL evidence.

## Build prompt

> In crate `fid-uv`, add `cfg(target_os = "macos")` module `mac` and `cfg(windows)` module `win`, each implementing `UvProvider`. macOS: for every call create a fresh `LAContext` with reuse duration 0, set a fixed localized reason naming the action and key label, and return a single-use `UvToken` carrying that context for the keystore's `SecKeyCreateSignature` call; method `macos_local_auth`. Windows: for every call run Windows Hello `UserConsentVerifier` with a fixed message naming the action and key label, return a token only on `Verified`; method `windows_hello`. Map cancel, fail and not-configured to `Cancelled`, `Failed`, `Unavailable`. Put OS calls behind a small trait so tests can substitute doubles. Tests: AC-3, AC-4 in CI; AC-1, AC-2 as manual hardware checks. Constraints: CONSTITUTION §8, REQ-TRUST.SIGN-005 (fresh verification per operation, no caching).

## Open questions

- On Windows the Hello check gates the token in software, not in the TPM key's policy [unverified alternatives]; if a binding mechanism for P-256 CNG keys is found it should replace this.

## Children

_Leaf: none._
