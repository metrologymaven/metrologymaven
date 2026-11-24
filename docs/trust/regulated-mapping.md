# Regulated Governance Profile: Control Mapping

Licence: CC-BY-4.0 (original project documentation).

## Compliance Disclaimer

The Regulated governance profile provides technical controls designed to support
electronic records, electronic signatures, audit trails, and release gating.
Using the Regulated profile supports, but does **not** by itself establish,
regulatory compliance with 21 CFR Part 11, EU GMP Annex 11, or any other
regulatory framework. Procedural controls, quality management procedures,
computerised-system validation (IQ, OQ, and PQ), and final compliance
determinations remain solely the responsibility of the user organisation.

## Scope and Citation Policy

- This document maps technical controls provided by Metrology Maven's
  `regulated` built-in governance profile (`TRUST.PROFILE.DEFS`) to related
  regulatory frameworks by **designator and section number only**. No
  regulatory text is quoted or reproduced here.
- Every section number in this initial table is marked `draft` and
  `[unverified]` pending sign-off by a reviewer with relevant regulatory
  experience. Per `REQ-TRUST.PROFILE.MAP-004`, a row may transition from
  `draft` to `reviewed` only in a pull request naming a reviewer with relevant
  regulatory experience.
- The machine-readable source of truth is
  [`regulated-mapping.csv`](regulated-mapping.csv), checked by
  `ci/trust/check-mapping` to verify that every cited specification requirement
  ID exists in `spec/**/NODE.md` and every row carries a valid `review_status`.

## Control Mapping Table

| Control ID | Project Description | Spec Requirement IDs | Regulation Designator | Section `[unverified]` | Review Status |
|---|---|---|---|---|---|
| `esig_manifestation` | Embeds signer printed name, UTC timestamp with local offset, meaning code and label, and key fingerprint inside the signed statement. | `REQ-TRUST.ESIG-003`, `REQ-TRUST.ESIG.BIND-002` | 21 CFR Part 11 | 11.50 | `draft` |
| `esig_meaning` | Uses a closed, machine-readable catalogue of built-in and tenant-prefixed signature meaning codes with explicit display labels. | `REQ-TRUST.ESIG-008`, `REQ-TRUST.ESIG.MEANING-001`, `REQ-TRUST.ESIG.MEANING-002` | 21 CFR Part 11 | 11.50(a)(3) | `draft` |
| `esig_linkage` | Cryptographically binds each DSSE signature envelope to the Result Package statement digest and records the digest in the journal. | `REQ-TRUST.ESIG-004`, `REQ-TRUST.ESIG.BIND-001`, `REQ-TRUST.ESIG.BIND-004` | 21 CFR Part 11 | 11.70 | `draft` |
| `esig_sequence` | Enforces an ordered chain of up to eight signatures where each signature commits to all prior signatures and validates required meaning order. | `REQ-TRUST.ESIG-007`, `REQ-TRUST.ESIG.MULTI-001`, `REQ-TRUST.ESIG.MULTI-002`, `REQ-TRUST.ESIG.MULTI-004` | EU GMP Annex 11 | 14 | `draft` |
| `esig_ceremony_human` | Restricts signing to an attended human session with full preview before confirmation, unadopted-agent refusal, and refusal logging. | `REQ-TRUST.ESIG-001`, `REQ-TRUST.ESIG-002`, `REQ-TRUST.ESIG-005`, `REQ-TRUST.ESIG-006`, `REQ-TRUST.ESIG.CEREMONY-001`, `REQ-TRUST.ESIG.CEREMONY-003` | 21 CFR Part 11 | 11.200(a) | `draft` |
| `audit_trail_chain` | Maintains a hash-chained, append-only Record Journal with UTC timestamps, principal commitments, AI-assistance levels, and input/output digests. | `REQ-CORE.JRN.ENTRY-001`, `REQ-CORE.JRN.ENTRY-002`, `REQ-CORE.JRN.ENTRY-005`, `REQ-CORE.JRN.UNDO-006` | 21 CFR Part 11 | 11.10(e) | `draft` |
| `audit_trail_annex11` | Protects released journal branches permanently and preserves superseded records visibly with reason commitments. | `REQ-CORE.JRN.ENTRY-001`, `REQ-CORE.JRN.BR.NAME-004`, `REQ-CORE.JRN.BR.NAME-005`, `REQ-CORE.JRN.UNDO-006` | EU GMP Annex 11 | 9 | `draft` |
| `release_gating` | Blocks marking a Result Package released under Regulated until every required signature meaning is applied in configured order. | `REQ-TRUST.PROFILE.DEFS-003`, `REQ-TRUST.PROFILE.GATE-002`, `REQ-TRUST.PROFILE.GATE.RELEASE-001`, `REQ-TRUST.PROFILE.GATE.RELEASE-002` | EU GMP Annex 11 | 14 | `draft` |
| `hardware_backed_keys` | Disables software-only signing keys under Regulated and creates non-extractable hardware-backed keys where supported. | `REQ-TRUST.PROFILE.DEFS-003`, `REQ-TRUST.PROFILE.GATE.QUERY-004`, `REQ-TRUST.SIGN.KEYSTORE.CMDS-001`, `REQ-TRUST.SIGN.KEYSTORE.MAC-001` | 21 CFR Part 11 | 11.200(a)(1) | `draft` |
| `user_verification` | Requires fresh OS or hardware-token user verification for every signing operation with no cached reuse window and logs refusals. | `REQ-TRUST.SIGN.UV-003`, `REQ-TRUST.SIGN.UV-004`, `REQ-TRUST.SIGN.UV.GATE-001`, `REQ-TRUST.SIGN.UV.GATE-006` | 21 CFR Part 11 | 11.200(a)(1)(ii) | `draft` |
| `profile_stamping` | Records the canonical SHA-256 digest of the active governance profile in every journal session header and exported Result Package. | `REQ-TRUST.PROFILE.DEFS-006`, `REQ-TRUST.PROFILE.GATE-004`, `REQ-TRUST.PROFILE.GATE.STAMP-001` | EU GMP Annex 11 | 12 | `draft` |
| `program_release_separation` | Requires a different human principal to release a measurement program than the person or agent who authored it under Regulated. | `REQ-TRUST.PROFILE.DEFS-002`, `REQ-TRUST.PROFILE.DEFS-003` | EU GMP Annex 11 | 12.4 | `draft` |

## Review Checklist for Promoting Rows to `reviewed`

Before changing any row's `review_status` from `draft` to `reviewed`:

1. Confirm that the cited designator and section number accurately correspond to
   the described technical control.
2. Confirm that no regulatory text longer than a section title is quoted in
   either `regulated-mapping.md` or `regulated-mapping.csv`.
3. Record the reviewer's name and regulatory-review qualification in the pull
   request description.
4. Run `python ci/trust/check-mapping --self-test` and verify exit code `0`.
