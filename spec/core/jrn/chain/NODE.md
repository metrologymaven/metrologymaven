---
id: CORE.JRN.CHAIN
title: Append, hash chain, checkpoints and verification
level: 3
parent: CORE.JRN
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-CORE.JRN-001, REQ-CORE.JRN-006, REQ-CORE.JRN-007, REQ-CORE.JRN-015, REQ-CORE.JRN-017]
covers: []
depends_on: [CORE.STO.SQL, CORE.STO.CAS, CORE.JRN.ENTRY]
cli: [fid journal list-entries, fid journal verify-chain]
mcp: [journal_list_entries, journal_verify_chain]
journal: [journal.list_entries@1, journal.verify_chain@1]
updated: 2026-11-17
---

# CORE.JRN.CHAIN: Append, hash chain, checkpoints and verification

## Purpose

The physical journal. CHAIN appends entries in one linear hash chain per stream over append order (whatever their branch), makes each append durable before returning, offers no way to modify or delete an entry, produces journal heads and checkpoints for TRUST to sign, and verifies the chain, checkpoints and referenced blobs on demand.

## Scope

**In:**
- Atomic, durable append through a `CORE.STO` transaction
- Sequence numbering and previous-hash linking
- Exactly one Record Journal entry per committed W1, W2, W3, D1, M1 or S1 command
- No modify or delete API; reliance on STO's database-level enforcement
- Heads per branch (derived cache) and checkpoints at signature, export and project close (P0: signed linear head; P1b: Merkle checkpoints)
- `journal.verify_chain@1` and `journal.list_entries@1`
- Throughput budget

**Out** (and which node owns it instead):
- Entry schema and hashing rules (owned by `CORE.JRN.ENTRY`)
- Which stream an entry goes to (owned by `CORE.JRN.STREAM`)
- Signing checkpoints (owned by `TRUST`)
- Database files and triggers (owned by `CORE.STO`)

## Requirements

- **REQ-CORE.JRN.CHAIN-001** When a W1, W2, W3, D1, M1 or S1 command commits, the journal shall append exactly one Record Journal entry.
- **REQ-CORE.JRN.CHAIN-002** Each appended entry shall carry a sequence number one greater than the previous entry of the same stream and the hash of that previous entry.
- **REQ-CORE.JRN.CHAIN-003** When an append returns success, the entry and every blob it references shall already be durable.
- **REQ-CORE.JRN.CHAIN-004** The journal API shall offer no operation that modifies or deletes an appended entry.
- **REQ-CORE.JRN.CHAIN-005** When a signature, an export or a project close occurs, the journal shall produce a checkpoint holding the stream, sequence, head hash and entry count, for signing by TRUST.
- **REQ-CORE.JRN.CHAIN-006** When `journal.verify_chain@1` runs, the journal shall report the first entry whose hash, previous-hash link or referenced blob digest fails, or report success with the number of entries checked.
- **REQ-CORE.JRN.CHAIN-007** When `journal.verify_chain@1` finds a checkpoint, it shall check that the checkpoint's head hash matches the chain at that sequence.
- **REQ-CORE.JRN.CHAIN-008** `journal.list_entries@1` and `journal.verify_chain@1` shall be side-effect class R0.
- **REQ-CORE.JRN.CHAIN-009** The journal shall append at least 1,000 entries per second with durable commit on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `Journal::append(draft) -> EntryHash`, `Journal::head(branch)`, `Journal::entries(range)`, `Journal::checkpoint() -> Checkpoint`, `Journal::verify(opts) -> VerifyReport`.
- **Commands:** `journal.list_entries@1` (R0), `journal.verify_chain@1` (R0). CLI `fid journal list-entries | verify-chain`.
- **Consumes:** `CORE.STO` (transactions, append-only tables, blob store), `CORE.JRN.ENTRY`.

## Decomposition hints

APPEND (atomic durable append and sequence), HEAD (head cache and checkpoints), VERIFY (verify command), LIST (list command and filters), PERF (throughput benchmark and group commit).

## Open questions

- Merkle checkpoints in the C2SP tlog-checkpoint format are P1b [unverified that the format fits]; the P0 checkpoint layout should leave room for them.
- The parent's interface names `journal verify`; this child uses `journal.verify_chain@1` to follow the `verb_object` rule.
- **Applied D-031:** both commands are R0, so they are written only to the Activity Log (when recording or as a program step) and never to the Record Journal; the `journal:` field names their Activity Log events.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.JRN.CHAIN.APPEND | Atomic durable append, sequence and immutability (leaf) | 001 002 003 004 009 | none |
| CORE.JRN.CHAIN.CKPT | Branch heads and checkpoints (leaf) | 005 | none |
| CORE.JRN.CHAIN.VERIFY | Verify and list commands (leaf) | 006 007 008 | none |
