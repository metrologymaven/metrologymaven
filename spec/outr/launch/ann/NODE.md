---
id: OUTR.LAUNCH.ANN
title: Announcement plan and drafts
level: 3
parent: OUTR.LAUNCH
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-004, REQ-OUTR.LAUNCH-005]
covers: []
depends_on: [OUTR.LAUNCH.GATE, OUTR.BRAND.LINT, OUTR.COMM.CHAN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.LAUNCH.ANN: Announcement plan and drafts

## Purpose

The announcement side of a launch: which channels carry each milestone announcement, what is posted there, who owns each post, and the drafts themselves, all checked by the claim lint and gated on the milestone gate report. It is separate because posting is the moment a claim becomes public, and it is done by people.

## Scope

**In:**
- Announcement channel list per milestone with material and owner
- Draft texts kept in the repository
- Pre-posting checks: gate result is ready, drafts pass the claim lint
- Timing relative to the release (after artifacts and docs are live)

**Out** (and which node owns it instead):
- Gate evaluation (owned by `OUTR.LAUNCH.GATE`)
- Channel inventory (owned by `OUTR.COMM.CHAN`)
- Videos and tutorials linked from announcements (owned by `OUTR.CONTENT`)
- Paid promotion (business documents outside this tree)

## Requirements

- **REQ-OUTR.LAUNCH.ANN-001** The launch plan shall name, for each milestone, every announcement channel, the material to post there and the person or role who owns the post.
- **REQ-OUTR.LAUNCH.ANN-002** Every announcement draft shall be kept in the repository and shall pass the OUTR.BRAND claim lint with zero errors before posting.
- **REQ-OUTR.LAUNCH.ANN-003** If the gate report of the release is not ready, then the pre-posting check shall fail for every draft that uses the milestone's name.
- **REQ-OUTR.LAUNCH.ANN-004** An announcement shall be posted only after the release's download page entry and docs edition are published, and each draft shall link both.
- **REQ-OUTR.LAUNCH.ANN-005** Announcements shall be posted by people; an agent may draft them but shall not post them (posting is X1 egress under CONSTITUTION §3).

## Interfaces

- **Offers:** announcement plan entries and drafts; the pre-posting check.
- **Consumes:** OUTR.LAUNCH.GATE result; OUTR.BRAND.LINT; OUTR.COMM.CHAN inventory; OUTR.SITE.DL and OUTR.DOCS.FRAME links.

## Decomposition hints

CHLIST (channel plan), DRAFT (draft storage), CHECK (pre-posting check).

## Open questions

- None beyond the parent's.

## Children

_None yet: status `open`._
