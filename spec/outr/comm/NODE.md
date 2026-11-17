---
id: OUTR.COMM
title: Community channels and adoption metrics
level: 2
parent: OUTR
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-006]
covers: []
depends_on: [OUTR.SITE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.COMM: Community channels and adoption metrics

## Purpose

The public places where users and contributors talk (forum, chat, issue tracker, mailing list) with their moderation rules, and the measurement of adoption from public signals and opt-in surveys only. These belong together because the channels are the main source of the public signals that adoption metrics count, and both must respect the no-telemetry rule (CONSTITUTION C7).

## Scope

**In:**
- Choice criteria and inventory of community channels; each channel's purpose
- Moderation rules, code-of-conduct enforcement procedure, moderator roster and escalation
- Channel hygiene: privacy of members, archives, spam handling
- Adoption metrics: downloads, repository stars and forks, forum and chat activity, contributor counts, opt-in surveys
- Quarterly adoption report

**Out** (and which node owns it instead):
- Product telemetry (none by default; any opt-in in-product telemetry belongs to `CORE`/`PLAT`, not here)
- Governance policy and the code-of-conduct text (GOVERNANCE.md, D-003)
- Contributor ladder and RFC tooling (owned by `ECO`)
- Launch announcements (owned by `OUTR.LAUNCH`)
- Vendor and university program channels (owned by `OUTR.PROG`)

## Requirements

- **REQ-OUTR.COMM-001** The project shall list every official community channel on the website with its purpose, its moderation rules and how to report abuse.
- **REQ-OUTR.COMM-002** Every official channel shall be readable without an account, or shall have a public read-only archive.
- **REQ-OUTR.COMM-003** When a moderation report is filed, a moderator shall acknowledge it within 72 hours [unverified target], and the outcome category (no action, warning, removal, ban) shall be recorded in a moderation log that is not public but is auditable by the steering group.
- **REQ-OUTR.COMM-004** Adoption metrics shall be computed only from public signals (download counts, repository stars and forks, public channel activity, contributor counts) and from opt-in survey responses, and shall never use data collected by the product.
- **REQ-OUTR.COMM-005** Each metric shall have a written definition stating its source, its collection method and its known biases (for example, mirror downloads not counted).
- **REQ-OUTR.COMM-006** The project shall publish an adoption report within 30 days after the end of each calendar quarter, showing every defined metric for that quarter and the previous three.
- **REQ-OUTR.COMM-007** Opt-in surveys shall collect no personal data beyond what the respondent explicitly enters, shall state their purpose and retention period, and shall publish only aggregates with at least 10 respondents per reported cell [unverified threshold].
- **REQ-OUTR.COMM-008** If a channel provider adds tracking that cannot be disabled, then the project shall record that fact on the channel listing and keep a tracker-free alternative available.

## Interfaces

- **Offers:** the channel directory and moderation rules (rendered by OUTR.SITE); metric definitions; the quarterly adoption report.
- **Consumes:** ECO release download counts; repository host public statistics; GOVERNANCE.md code of conduct; OUTR.LAUNCH milestones (to annotate reports).

## Decomposition hints

CHAN (channel inventory and setup), MOD (moderation procedure and log), METRIC (metric definitions and collection), SURVEY (opt-in surveys), REPORT (quarterly report).

## Open questions

- Which hosted channel providers meet REQ-OUTR.COMM-002 and REQ-OUTR.COMM-008 is not yet researched [unverified].
- The 72-hour and 10-respondent figures are proposals for maintainers to confirm.

## Children

_None yet: status `open`._
