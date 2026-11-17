---
id: TRUST
title: Signing and independent verification
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-018]
covers: []
depends_on: [CORE, UNC]
cli: [fid verify *, fid sign *]
mcp: [verify_*]
journal: [sign.*]
updated: 2026-11-17
---

# TRUST: Signing and independent verification

## Purpose

Makes results checkable by anyone:
- signed Result Packages (standard DSSE);
- a free, open-source verifier (`fid-verify`) that re-checks signatures, journal chains and, where points are included, recomputes results with the reference kernel;
- the human e-signature flow for regulated users.

The Expert Attestation Network from the concept docs is **not** part of the open-source core. It may come back as a separate project (decision D-001 context).

## Scope

**In:**
- Result Package format
- DSSE signing with OS-keystore keys
- `fid-verify` (CLI + local stdio MCP)
- Journal checkpoint verification
- Verifier replay with the WASM reference kernel
- Governance profiles (Lean, Regulated, Assistive-only)
- Signing UI hooks with signature meanings

**Out:**
- Hosted transparency services and expert attestation (out of scope for the open core; reconsider by decision)

## Requirements

- **REQ-TRUST-001** The system shall export signed Result Packages in standard DSSE envelopes that off-the-shelf in-toto tooling can verify.
- **REQ-TRUST-002** `fid-verify` shall be free and open source, never licence-checked, and shall verify signatures, journal hash chains and checkpoints offline.
- **REQ-TRUST-003** When a Result Package includes points, `fid-verify` shall recompute the results with the WASM reference kernel and report `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`.
- **REQ-TRUST-004** Signing shall be a human-only S1 action released by OS user verification. Agents shall never hold or trigger signatures.
- **REQ-TRUST-005** Tenants shall choose a governance profile (Lean, Regulated, Assistive-only) that changes GUI gating but never the command class taxonomy.

## Interfaces

- **Offers:** the `fid-verify` binary; the Result Package spec; `verify.*` and `sign.*` commands.
- **Consumes:** CORE journal, UNC/KRN reference kernel.

## Decomposition hints

PKG (Result Package format), SIGN (DSSE + keystore + meanings), VERIFY (fid-verify CLI + MCP), REPLAY (verifier replay), PROFILE (governance profiles).

## Split rationale

The split follows the **life of a Result Package**: what it contains (PKG), how it is cryptographically signed (SIGN), why and by whom a person signs (ESIG), which policy decides what signing is required (PROFILE), and the three independent checks a verifier runs on it (CHAIN, REPLAY), composed into the free `fid-verify` tool (VERIFY). Cryptography is kept apart from the human ceremony so the crypto surface stays small and is shared by signer and verifier; each verifier check is its own subsystem so it can be tested against its own corpus and reused by the main CLI.

Alternatives considered:
- **Split by binary** (signer side vs. `fid-verify` side). Rejected: signature checking and the package reader are shared, and would be duplicated or owned ambiguously.
- **Folding ESIG into SIGN** (the decomposition hint). Rejected: regulated-signature semantics (meanings, manifestation, multi-signer) change for different reasons than key custody and DSSE encoding.
- **Folding CHAIN into VERIFY.** Rejected: chain checking is a self-contained, independently testable re-implementation that should not share code with CORE's writer.

## Open questions

- Should Part 11 controls (Regulated profile) be in the open core? See D-001. `TRUST.PROFILE` assumes yes.
- CONSTITUTION §7 puts the verifier in P0, but this domain is P1a. A chain-only verifier may be needed earlier; a phase move needs a steering decision.
- Several R0 tools (`sign_list_keys`, `sign_list_meanings`, `sign_show_profile`) fall outside this node's `mcp: [verify_*]` pattern. Either widen the pattern to include read-only `sign_*` tools or rename them under `verify`; maintainers to choose at review.
- CORE owns the replay engine for all modes; `TRUST.REPLAY` specifies a standalone verifier-mode replayer. The boundary should be confirmed with CORE when it is split.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.PKG | Result Package format | 001 003 | none |
| TRUST.SIGN | DSSE signing and key custody | 001 002 004 | none |
| TRUST.ESIG | Human e-signature ceremony and meanings | 004 | none |
| TRUST.CHAIN | Journal chain and checkpoint verification | 002 | none |
| TRUST.REPLAY | Verifier replay with the reference kernel | 003 | none |
| TRUST.VERIFY | fid-verify binary and verification report | 002 003 | none |
| TRUST.PROFILE | Governance profiles | 005 | none |
