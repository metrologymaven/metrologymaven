---
id: TRUST.CHAIN
title: Journal chain and checkpoint verification
level: 2
parent: TRUST
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-TRUST-002]
covers: []
depends_on: [CORE, TRUST.PKG, TRUST.SIGN]
cli: [fid verify check-chain]
mcp: [verify_check_chain]
journal: []
updated: 2026-11-16
---

# TRUST.CHAIN: Journal chain and checkpoint verification

## Purpose

Independent, offline checking of the journal evidence inside a Result Package: that the hash chain is unbroken, that signed checkpoints match it, and that every ResultRecord's journal entry hash points into the verified chain. CORE writes the journal; this node is the verifier-side re-implementation of the checks, kept separate so the verifier does not trust the code that wrote the data.

## Scope

**In:**
- Recomputing entry hashes (RFC 8785 canonical JSON + SHA-256) from the packaged journal segment
- Checking predecessor links and sequence numbers
- Checkpoint format check: checkpoint digest over a chain head, optionally signed (via `TRUST.SIGN`)
- Linking each ResultRecord's journal entry hash to an entry in the verified segment
- Detecting truncation, reordering, insertion and gaps
- Typed findings for each failure, with entry index

**Out** (and which node owns it instead):
- Writing the journal and creating checkpoints (`CORE`)
- Package layout (`TRUST.PKG`)
- Signature cryptography (`TRUST.SIGN`)
- Recomputing results (`TRUST.REPLAY`)
- Hosted transparency logs (out of scope for the open core)

## Requirements

- **REQ-TRUST.CHAIN-001** The chain checker shall recompute every entry hash in a packaged journal segment and report `VALID` only if each equals the stored hash and links to its predecessor.
- **REQ-TRUST.CHAIN-002** If any entry is modified, removed, inserted or reordered, then the checker shall report `BROKEN` with the index of the first failing entry.
- **REQ-TRUST.CHAIN-003** When a package contains a checkpoint, the checker shall confirm the checkpoint digest equals the recomputed chain head at the checkpoint index.
- **REQ-TRUST.CHAIN-004** When a checkpoint is signed, the checker shall verify that signature with `TRUST.SIGN` and report the signer fingerprint.
- **REQ-TRUST.CHAIN-005** The checker shall report each ResultRecord whose journal entry hash is absent from the verified segment as `UNLINKED`.
- **REQ-TRUST.CHAIN-006** The checker shall verify a segment of 1,000,000 entries in under 30 s on the reference CI machine with no network access [unverified].
- **REQ-TRUST.CHAIN-007** The checker shall share no hashing or canonicalisation code with the journal writer beyond the published algorithm definitions, and a cross-check test shall run both against the same golden journals.

## Interfaces

- **Offers:** `verify.check_chain` (R1) returning typed findings; a library used by `TRUST.VERIFY`.
- **Consumes:** journal entry format and checkpoint format from `CORE`; package reader from `TRUST.PKG`; signature check from `TRUST.SIGN`.

## Decomposition hints

CANON (independent RFC 8785 implementation + golden vectors), LINKS (chain walk + findings), CKPT (checkpoint check), RESLINK (ResultRecord linkage).

## Split rationale

The split follows **the order a verifier establishes trust in the journal**: bytes to hashes (CANON), hashes to an unbroken chain (LINKS), chain to signed checkpoints (CKPT), and chain to the results that claim to come from it (RESLINK). LINKS also owns the `verify.check_chain@1` command that composes the other three, so there is one entry point and no separate orchestration leaf.

Alternatives considered:
- **One leaf for the whole chain check.** Rejected: the independent canonicaliser alone is an M-sized, differential-fuzzed unit.
- **Command in its own child.** Rejected: composing three library calls into one report is too small to stand alone.

## Open questions

- REQ-TRUST.CHAIN-007 asks for an independent canonicaliser (`TRUST.CHAIN.CANON`); if maintainers prefer one shared audited crate, record that as a decision.
- The checkpoint format is owned by CORE.JRN (still open); `TRUST.CHAIN.CKPT` assumes format version, index and head digest, and P0 signed linear heads only.
- CONSTITUTION §7 lists the verifier in P0 while this subsystem is P1a; see the phase decision referenced from `TRUST.VERIFY`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| TRUST.CHAIN.CANON | Independent RFC 8785 canonicaliser and golden vectors | 007 | none |
| TRUST.CHAIN.LINKS | Chain walk, findings and check-chain command | 001 002 006 | none |
| TRUST.CHAIN.CKPT | Checkpoint check and signed-checkpoint verification | 003 004 | none |
| TRUST.CHAIN.RESLINK | ResultRecord to journal linkage | 005 | none |
