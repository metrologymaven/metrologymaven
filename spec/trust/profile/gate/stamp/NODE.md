---
id: TRUST.PROFILE.GATE.STAMP
title: Profile digest stamping of sessions and packages
level: 4
parent: TRUST.PROFILE.GATE
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-TRUST.PROFILE.GATE-004]
covers: []
depends_on: [TRUST.PROFILE.GATE.QUERY, CORE.JRN.ENTRY]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# TRUST.PROFILE.GATE.STAMP: Profile digest stamping of sessions and packages

## Purpose

Makes the governing profile visible in every record: it supplies the active profile digest for each journal `session_header` entry, and a `ProfileStamp` (name, digest, canonical document bytes) that the package content assembler embeds as `profile/profile.json`. A mid-session change is already journaled by `sign.set_profile@1`; this leaf checks that the records agree with it.

## Scope

**In:**
- `ProfileStamp` for the exporter
- The session-header provider that fills `profile_digest`
- A consistency check: profile digest in force for any journal sequence

**Out:**
- The `sign.set_profile@1` event (`TRUST.PROFILE.SELECT`)
- Writing the header entry (`CORE.JRN.ENTRY`) and the package (`TRUST.PKG.CONTENT.SELECT`)

## Requirements

- **REQ-TRUST.PROFILE.GATE.STAMP-001** When a journal session starts, the provider shall supply the active profile digest for the `session_header` entry's `profile_digest` field.
- **REQ-TRUST.PROFILE.GATE.STAMP-002** `stamp()` shall return the active profile name, digest and RFC 8785 canonical document bytes whose SHA-256 equals the digest.
- **REQ-TRUST.PROFILE.GATE.STAMP-003** `digest_at(sequence)` shall return the digest from the latest `session_header` or `sign.set_profile@1` event at or before that sequence.
- **REQ-TRUST.PROFILE.GATE.STAMP-004** If a session header's `profile_digest` disagrees with the digest implied by the preceding events of the same session, then `check_consistency` shall report `profile.stamp_mismatch` with the sequence.

## Acceptance criteria

- **AC-1** Given Regulated active, when a session starts, then the header's `profile_digest` equals the pinned Regulated digest.
- **AC-2** Given `stamp()` under Lean, then SHA-256 of the returned bytes equals the returned digest and equals the pinned Lean digest.
- **AC-3** Given fixture journal `fixtures/profile/stamp/change-mid-session.jsonl` (Lean header, `sign.set_profile@1` to Regulated at 12), then `digest_at(11)` is Lean and `digest_at(12)` is Regulated.
- **AC-4** (error path) Given a header whose digest was edited, then `profile.stamp_mismatch` at that sequence.

## Design

- **Registry command(s):** none.
- **Data structures:** `ProfileStamp { name, digest, document: Vec<u8> }`; `SessionHeaderProvider` impl.
- **Algorithm:** stamp from the `query` snapshot; `digest_at` walks the segment backwards to the nearest header or set-profile event.
- **Errors:** `profile.stamp_mismatch`.
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-profile`, module `stamp`.

## Test fixtures

`fixtures/profile/stamp/*.jsonl`, synthetic journals built with CORE.JRN's entry format.

## Build prompt

> In crate `fid-profile`, add module `stamp`: `pub fn stamp(q: &dyn ProfileQuery) -> ProfileStamp` (name, digest, RFC 8785 canonical document bytes; assert-free check that SHA-256(bytes) == digest, else `profile.stamp_mismatch`), a session-header provider that returns the active digest for the journal's `session_header.profile_digest` field, `digest_at(entries, seq)` (latest header or `sign.set_profile@1` at or before `seq`) and `check_consistency(entries)` reporting `profile.stamp_mismatch` with the sequence. Tests: AC-1..AC-4. Constraints: CONSTITUTION §6, §8, D0.

## Open questions

- CORE.JRN.ENTRY's `session_header` has no `profile_digest` field yet (REQ-CORE.JRN.ENTRY-006 lists registry and kernel manifest digests and app version); CORE must add it [cross-domain, unresolved].

## Children

_Leaf: none._
