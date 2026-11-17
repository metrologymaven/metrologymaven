---
id: CORE.JRN
title: Semantic journal: hash chain, streams, branches and undo
level: 2
parent: CORE
kind: subsystem
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-CORE-003, REQ-CORE-004, REQ-CORE-008]
covers: [GOM.PRJ.06, PW.PRJ.04]
depends_on: [CORE.STO]
cli: [fid journal log, fid journal verify, fid journal undo, fid journal redo, fid journal *-branch]
mcp: [journal_log, journal_verify, journal_undo, journal_redo, journal_*_branch]
journal: [journal.*]
updated: 2026-11-16
---

# CORE.JRN: Semantic journal

## Purpose

The append-only, hash-chained record of every committed command (CONSTITUTION §6 layer 1). JRN defines the entry model, appends entries atomically, keeps the physical hash chain and the logical state DAG, and provides branches, undo and redo as head moves. It also produces signed-ready journal heads (checkpoints) that TRUST can sign and the verifier can check. It is separate from replay (`CORE.RPL`), which reads the journal, and from storage (`CORE.STO`), which only provides durable bytes.

## Scope

**In:**
- Entry model: sequence, previous hash, entry hash, branch, logical parent, time (UTC, monotonic, clock-sync state), principal kind plus committed identity, AI-assistance level, command ID and major, class, seed, input/output digests, kernel and registry manifest digests, trace IDs
- Canonical hashing: RFC 8785 JSON, SHA-256; 64-bit integers as decimal strings; bit-significant f64 as IEEE hex; non-finite values rejected
- Salted commitments for detachable fields (arguments, event bodies, reasons, identities)
- Streams: Record Journal (W1–W3, D1, M1, S1, head moves, branch operations) and the local Activity Log (R0/R1, job lifecycle)
- Physical chain over append order and logical DAG over states
- Branches: create, list, diff (entity by entity), promote; protected branches; agent branches `agent/<client>/<n>`; promotion request
- Undo and redo as `HeadMove` entries; undo tree
- Supersession of released records (never undo)
- Journal heads and checkpoints for signing (signing itself is TRUST)
- `journal verify`: chain, checkpoints and blob digests
- Session header carrying registry manifest and kernel manifest digests

**Out** (and which node owns it instead):
- Re-executing or reconstructing state from entries (owned by `CORE.RPL`)
- Durable files, SQLite and blob storage (owned by `CORE.STO`)
- Deciding which branch a command commits to (owned by `CORE.BUS`)
- Signing checkpoints and human assertions (owned by `TRUST`)
- Presentation track and video (owned by `VIZ`)
- Instrument I/O transcripts (owned by `HAL`)
- Access Ledger for external principals (deferred to P1b; see Open questions)

## Requirements

- **REQ-CORE.JRN-001** When a W1, W2, W3, D1, M1 or S1 command commits, the journal shall append exactly one Record Journal entry.
- **REQ-CORE.JRN-002** Each entry hash shall be SHA-256 over the RFC 8785 canonical JSON of its header, and the header shall include the previous entry's hash.
- **REQ-CORE.JRN-003** The journal shall encode every 64-bit integer in a hashed structure as a decimal string and every bit-significant f64 as the hexadecimal form of its IEEE 754 bits.
- **REQ-CORE.JRN-004** If an entry contains a NaN or infinite number, then the journal shall refuse to append it and the commit shall fail.
- **REQ-CORE.JRN-005** The journal shall store every detachable field (arguments, event bodies, reason, principal identity) in the header only as a salted SHA-256 commitment with a 128-bit salt.
- **REQ-CORE.JRN-006** The journal shall reject any attempt to modify or delete an appended entry.
- **REQ-CORE.JRN-007** When `fid journal verify` runs, the journal shall report the first entry whose hash, previous-hash link or referenced blob digest does not verify, or report success.
- **REQ-CORE.JRN-008** When undo is invoked, the journal shall append a `HeadMove` entry and shall not remove any entry.
- **REQ-CORE.JRN-009** When a command commits after an undo, the journal shall fork the logical DAG so that the undone states remain reachable by redo-tree navigation.
- **REQ-CORE.JRN-010** The journal shall support named branches, and `diff` between two branches shall list entity differences keyed by stable entity ID.
- **REQ-CORE.JRN-011** Promotion into a protected branch shall be a W3 command that requires a human principal and a human assertion.
- **REQ-CORE.JRN-012** When an agent requests promotion of its branch, the journal shall create a promotion request record and shall not promote.
- **REQ-CORE.JRN-013** If an undo targets an entry that records a released (W3) record, then the journal shall refuse it; released records change only by a supersession entry that cites the superseded digest and a reason.
- **REQ-CORE.JRN-014** Every session shall begin with a header entry that records the registry manifest digest and the kernel manifest digest in force.
- **REQ-CORE.JRN-015** When an append returns success, the entry shall already be durable, so a process crash immediately afterwards does not lose it.
- **REQ-CORE.JRN-016** R0 and R1 calls by local principals shall be written to the Activity Log stream, not the Record Journal.
- **REQ-CORE.JRN-017** The journal shall append at least 1,000 entries per second with durable commit on reference hardware [unverified until measured].

## Interfaces

- **Offers:** `Journal::append(entry_draft) -> EntryHash` (atomic, durable), `Journal::head(branch)`, `Journal::entries(range)`, `Journal::branch_*`, `Journal::undo/redo`, `Journal::checkpoint() -> HeadDigest`, `Journal::verify()`; the entry JSON schema, which TRUST's verifier also implements.
- **Commands:** `journal.list_entries@1` (R0), `journal.verify_chain@1` (R0), `journal.undo@1` (W2), `journal.redo@1` (W2), `journal.create_branch@1` (W2), `journal.diff_branch@1` (R0), `journal.promote_branch@1` (W2 to unprotected, W3 to protected), `journal.request_promote_branch@1` (W1), `journal.supersede_record@1` (W3). Final names are fixed at leaf level.
- **Consumes:** `CORE.STO` (append-only tables, blob store, atomic transactions).

## Decomposition hints

ENTRY (entry model and canonical encoding), CHAIN (append, chain, verify), BR (branches, diff, promotion, protection), UNDO (head moves and undo tree), STREAM (Record Journal vs Activity Log), CKPT (heads and checkpoints for TRUST).

## Open questions

- **Owner assigned (level-2 review, accepted 2026-11-16):** CORE.JRN owns the Access Ledger stream format (P1b; baseline §4). Include it when this node is expanded.
- Access Ledger (R0/R1 by external principals) is specified in concept doc 03 for P1b. It is not in REQ-CORE-003; if maintainers want it in CORE rather than AUTO or TRUST, add it here by a later expansion.
- Activity Log chaining and compaction are P1b in the concept docs; P0 here has an unchained local log.
- Merkle (C2SP tlog-checkpoint) checkpoints are P1b [unverified that the format fits]; P0 is a signed linear head.

## Split rationale

The dividing principle is **bytes, then entries, then chain, then logic**: CANON turns values into canonical bytes; ENTRY defines what an entry holds and commits to; CHAIN appends entries durably in one physical chain and verifies it; BR and UNDO are the two kinds of logical-DAG operation (named branches with promotion, and head moves with supersession); STREAM decides which stream an entry belongs to and owns the Activity Log and the Access Ledger format (assigned to CORE.JRN by the level-2 review). CANON is a small, standalone crate shared with the registry and verifier, so it is a leaf.

Alternatives considered:
- **CKPT as its own child** (as the hints suggest). Rejected: checkpoints are heads of the physical chain and are verified by the same command, so they sit in CHAIN.
- **Undo inside BR.** Rejected: undo trees, barriers and supersession have their own commands and rules; BR is already large.
- **Access Ledger in AUTO or TRUST.** Rejected by the level-2 review assignment: the ledger shares the entry header and checkpoint anchoring with the journal, so the format lives here; the gateway that writes it stays in AUTO/TRUST.

The parent's single `promote` command at "W2 or W3" became two commands in BR, because one registry command has exactly one class.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.JRN.CANON | Canonical JSON encoding and digests (leaf) | 002 003 004 | none |
| CORE.JRN.ENTRY | Entry model, salted commitments and session header | 002 005 014 | none |
| CORE.JRN.CHAIN | Append, hash chain, checkpoints and verification | 001 006 007 015 017 | none |
| CORE.JRN.BR | Branches, diff, protection and promotion | 010 011 012 | none |
| CORE.JRN.UNDO | Undo, redo, undo tree and supersession | 008 009 013 | GOM.PRJ.06, PW.PRJ.04 |
| CORE.JRN.STREAM | Streams: Record Journal, Activity Log and Access Ledger format | 016 | none |
