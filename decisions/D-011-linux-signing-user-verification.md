---
id: D-011
title: Signing user verification where the OS offers none
status: accepted
raised_by: TRUST.SIGN split
blocks: [TRUST.SIGN.UV]
decide_by: before TRUST.SIGN.UV is built
updated: 2026-11-17
---

# D-011: Signing user verification where the OS offers none

## Question

REQ-TRUST.SIGN-005 requires a fresh OS user-verification prompt before every private-key operation. Windows and macOS provide one [unverified: Windows Hello / CNG UI policy, macOS LocalAuthentication with key access control]. Linux desktops have no single equivalent. What should the signer do on a platform with no OS user-verification API?

## Context

Signing is S1 and human-only (REQ-TRUST-004). Without a user-verification step, any process running as the user could sign through the keystore, which defeats the S1 guarantee. Options differ in security, usability on Linux shop-floor machines (a supported platform per PLAT), and how much the project implements itself. Blocks `TRUST.SIGN.UV`.

## Options

| Option | For | Against |
|---|---|---|
| A. Refuse to sign on platforms without OS user verification | Simplest; no home-grown authentication | Linux users cannot sign at all |
| B. Passphrase prompt owned by the app, key wrapped by a passphrase-derived key (Argon2id) in the Secret Service or a file | Works everywhere | The app implements authentication; passphrase entry can be scripted by a local attacker with UI access |
| C. Require an external authenticator: a PKCS#11 token or FIDO2 key with PIN and touch (presence check) | Strong, hardware-backed, user presence proven | Needs hardware on every Linux signing seat |
| D. C where a token is present, otherwise B, with the method recorded in the signature and shown by `fid-verify` | Usable and honest about strength | Two code paths; profiles must decide which is acceptable |

## Recommendation

D, with the Regulated profile requiring C (token with presence check) on Linux. Record `uv_method` in the signature statement so verifiers can see how the signer was verified.

## Decision

**Accepted (recommendation as written above), 2026-11-17, founder.**

D, with the Regulated profile requiring C (token with presence check) on Linux. Record `uv_method` in the signature statement so verifiers can see how the signer was verified.
