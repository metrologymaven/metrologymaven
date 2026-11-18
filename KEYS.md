# Metrology Maven public keys

**Status (2026-11): no signing keys exist yet, and no release or nightly build is signed.**
The table below is empty on purpose. Keys are created under the custody rules of spec node
`ECO.REL.SIGN.CUSTODY`, which is not built yet. Until a key is listed here, treat any
signature that claims to come from this project as unverifiable. To check a download today,
follow [VERIFY.md](VERIFY.md), which compares SHA-256 hashes only.

This file and the [`keys/`](keys/) directory are specified by `ECO.REL.SIGN.GUIDE`.

## Key classes

| Class | Signs | Trust |
|---|---|---|
| `release` | `SHA256SUMS` and the release statements of beta, stable and LTS releases, only after a maintainer approves that release | Full: a valid release signature means the project released this file |
| `nightly` | `SHA256SUMS` of nightly builds, unattended in CI | **Lower.** A nightly signature only says that the project's CI built the file. Nightly builds are not releases and must never be treated as one |
| `packaging` | Linux package repository metadata | Package managers check it; users normally do not |

## Formats

- Each key is published as `keys/<name>.pem`: the public key's SubjectPublicKeyInfo in PEM
  (`-----BEGIN PUBLIC KEY-----`).
- The release and nightly keys are also published in `keys/allowed_signers`, the OpenSSH
  allowed-signers format, for `ssh-keygen -Y verify` with namespace `fid-release` (planned:
  see VERIFY.md).
- **Fingerprint**: the lowercase hex SHA-256 of the key's SubjectPublicKeyInfo DER encoding,
  the same key ID used in DSSE envelopes (`TRUST.SIGN.DSSE`). It differs from the `SHA256:...`
  fingerprint that `ssh-keygen -l` prints, which hashes the SSH encoding of the key instead.
- Validity dates are inclusive, `YYYY-MM-DD`, UTC. Status is `active` (may sign) or
  `retired` (kept so that old signatures stay checkable; signs nothing new).

## Keys

| File | Class | Fingerprint | Valid from | Valid until | Status |
|---|---|---|---|---|---|

`cargo xtask rel keys-check` (run in CI) fails if a file in `keys/` is not in this table
(`KEYS-UNLISTED`), if a listed fingerprint does not match the file (`KEYS-FINGERPRINT`), if a
listed file is missing (`KEYS-MISSING`), or if an `active` key is past its validity
(`KEYS-EXPIRED-IN-USE`) or not yet valid (`KEYS-NOT-YET-VALID`).

## Key transitions

When a key is replaced, the project publishes a key-transition statement signed by the
outgoing key, so that anyone who trusts the old key can learn the new one. The format is
defined here; **no transition has happened and no statement exists yet**.

- File: `keys/transitions/<old fingerprint>-to-<new fingerprint>.dsse.json` (planned).
- Envelope: DSSE v1.0 (`TRUST.SIGN.DSSE`), signed by the outgoing key, payload type
  `application/vnd.fid.key-transition+json`.
- Payload: a JSON object, RFC 8785 canonical form, with exactly these fields:

| Field | Meaning |
|---|---|
| `old_keyid` | Fingerprint of the outgoing key (64 lowercase hex) |
| `new_keyid` | Fingerprint of the incoming key (64 lowercase hex) |
| `class` | `release`, `nightly` or `packaging` |
| `effective_from_version` | First version the new key signs (SemVer, for example `1.5.0`) |
| `published_at` | When the statement was published, RFC 3339 UTC (for example `2027-03-01T00:00:00Z`) |

The new key is listed in the table above (status `active`) before its first use, and the old
key's status changes to `retired`. The update checker (`ECO.REL.UPD`) and managed deployment
(`PLAT.DEPLOY.UPDATE`) read these statements. Verifying a statement needs a DSSE verifier
(`TRUST.SIGN.DSSE`), which is not built yet.

If a key is compromised rather than rotated, its owner cannot be trusted to sign a
transition; that case belongs to the incident process and is not covered by this format.
