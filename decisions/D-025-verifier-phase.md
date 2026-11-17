---
id: D-025
title: Phase of the verifier (P0 per constitution vs P1a for TRUST)
status: open
raised_by: TRUST level-2 split and TRUST.VERIFY expansion
blocks: []
decide_by: before P0 scope is frozen
updated: 2026-11-16
---

# D-025: Phase of the verifier (P0 per constitution vs P1a for TRUST)

## Question

CONSTITUTION §7 lists the verifier among the P0 foundations, but the TRUST domain and all its subsystems are tagged P1a. Should a minimal verifier start in P0, and if so which parts move?

## Context

Under D-006 a phase means "work starts", and a parent's phase is the earliest of its children. Moving any TRUST child to P0 moves TRUST to P0. The level-3 split makes a minimal P0 verifier concrete: `TRUST.CHAIN.CANON`, `TRUST.CHAIN.LINKS`, `TRUST.CHAIN.CKPT`, `TRUST.CHAIN.RESLINK`, `TRUST.PKG.MANIFEST`, `TRUST.PKG.READER` and `TRUST.VERIFY.CLI` (chain section only) need no keystore, no ceremony and no replay. P0 already includes the journal (CORE.JRN) whose golden journals the independent canonicaliser cross-checks. No node is blocked; the phase tags are wrong or right depending on this choice.

## Options

| Option | For | Against |
|---|---|---|
| A. Keep TRUST at P1a; amend CONSTITUTION §7 to drop the verifier from P0 | No schedule growth | The P0 promise of independent checking is lost; CORE.JRN's writer has no independent cross-check during P0 |
| B. Move the chain-only path (CANON, LINKS, CKPT, RESLINK, PKG.MANIFEST, PKG.READER, VERIFY.CLI) to P0; TRUST becomes P0 | Matches the constitution; catches journal bugs early | More P0 work; VERIFY.CLI must ship with signature and replay sections reported as `not_available` |
| C. Move only CANON and LINKS to P0 as a CI cross-check, no user-facing verifier until P1a | Smallest move; still cross-checks the journal | The constitution still overstates P0 |

## Recommendation

B. The chain-only path is roughly seven S/M leaves and makes the P0 claim true.

## Decision

_Open._ When decided: update the `phase:` of the moved nodes and of TRUST and its affected subsystems, or amend CONSTITUTION §7.
