---
id: OUTR.COMM.MOD
title: Moderation procedure and moderation log
level: 3
parent: OUTR.COMM
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.COMM-003]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.COMM.MOD: Moderation procedure and moderation log

## Purpose

How the code of conduct is enforced on official channels: how reports are filed and acknowledged, who the moderators are, how cases escalate, and the non-public moderation log that the steering group can audit. It is separate because it handles personal and sensitive data and must work the same way on every channel.

## Scope

**In:**
- Report intake routes (one per channel plus one private, channel-independent route)
- Acknowledgement target and the outcome categories (no action, warning, removal, ban)
- Moderator roster, conflict-of-interest rule and escalation to the steering group
- Moderation log format, access rules and retention
- Per-channel moderation rules text, written in our own words

**Out** (and which node owns it instead):
- The code-of-conduct text and governance (GOVERNANCE.md, D-003)
- Channel inventory and listing (owned by `OUTR.COMM.CHAN`)
- Spam filtering tools inside a provider (provider configuration, recorded by `OUTR.COMM.CHAN`)

## Requirements

- **REQ-OUTR.COMM.MOD-001** When a moderation report is filed through any official route, a moderator shall acknowledge it to the reporter within 72 hours [unverified target].
- **REQ-OUTR.COMM.MOD-002** Each closed report shall be recorded in the moderation log with date filed, date acknowledged, channel, outcome category (no action, warning, removal or ban) and handling moderator, and no other personal data beyond what the case needs.
- **REQ-OUTR.COMM.MOD-003** The moderation log shall not be public, and shall be readable by every steering-group member for audit.
- **REQ-OUTR.COMM.MOD-004** If a moderator is a party to or has a conflict of interest in a report, then a different moderator shall handle it, and the log shall record the hand-over.
- **REQ-OUTR.COMM.MOD-005** The project shall publish, per quarter, the count of reports per outcome category and the share acknowledged within the target, with no case details.
- **REQ-OUTR.COMM.MOD-006** Moderation log entries shall be deleted or anonymised after a retention period written in the procedure (initially 24 months [unverified]).

## Interfaces

- **Offers:** moderation procedure, rules text for each channel, abuse-report routes, quarterly moderation counts (for OUTR.COMM.REPORT).
- **Consumes:** GOVERNANCE.md code of conduct and steering-group roster (D-003).

## Decomposition hints

PROC (procedure and outcome categories), ROSTER (moderators and conflicts), LOG (log format, access and retention), STATS (quarterly counts).

## Open questions

- The 72-hour target and the retention period are proposals for maintainers to confirm.
- Where the private log is stored (which tool, which access control) must not leak data to third parties; it is a maintainer choice.

## Children

_None yet: status `open`._
