---
id: OUTR.LAUNCH
title: Launch plan, milestones and event kits
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-004]
covers: []
depends_on: [ECO, OUTR.BRAND, OUTR.CONTENT]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.LAUNCH: Launch plan, milestones and event kits

## Purpose

The written launch plan: what "alpha", "beta" and "1.0" mean for each focused app, the criteria that must be met before each is announced, the announcement channels, and the demo kits used at conferences and trade shows. It is separate because milestone criteria must be tied to the spec tree's phase gates and checked, not asserted, and because event kits are a self-contained deliverable that contributors can own.

## Scope

**In:**
- Launch plan document: milestones per app (alpha, beta, 1.0), each with criteria tied to spec-tree phases and parity numbers
- Milestone gate check: a report that shows, for each criterion, whether it is met, from generated data
- Announcement plan: channels, message drafts, timing relative to releases
- Conference and trade-show demo kits: a scripted demo, an example project and journal, offline installer set, hardware list, printed material
- Post-launch review notes

**Out** (and which node owns it instead):
- Phase definitions and phase moves (CONSTITUTION §7; changed only by decision)
- Release packaging and channels (owned by `ECO`)
- Tutorials and videos used in announcements (owned by `OUTR.CONTENT`)
- Booth costs, sponsorships, sales follow-up (business documents outside this tree)
- Adoption measurement after launch (owned by `OUTR.COMM`)

## Requirements

- **REQ-OUTR.LAUNCH-001** The project shall keep a written launch plan in the repository that defines alpha, beta and 1.0 for each focused app defined by D-004.
- **REQ-OUTR.LAUNCH-002** Each milestone shall list criteria that reference spec-tree phases, node statuses or parity-report numbers, and shall contain no criterion that cannot be checked from generated data or a named human sign-off.
- **REQ-OUTR.LAUNCH-003** When a release candidate is tagged, the gate check shall produce a report listing each criterion of the targeted milestone as met, not met or needs sign-off, generated from the spec tree and the release's parity report.
- **REQ-OUTR.LAUNCH-004** If any criterion of a milestone is not met, then the milestone shall not be announced under that milestone's name.
- **REQ-OUTR.LAUNCH-005** The launch plan shall name each announcement channel, the material to post there and its owner, and every announcement text shall pass the OUTR.BRAND claim lint before posting.
- **REQ-OUTR.LAUNCH-006** Each demo kit shall run fully offline from its own installer set, with a scripted demo that replays a published example journal to a result `IDENTICAL` to the recorded one.
- **REQ-OUTR.LAUNCH-007** Each demo kit shall run without motion-capable hardware, using the HAL simulators where a device is shown, unless a human operator and approval station are part of the kit.
- **REQ-OUTR.LAUNCH-008** Within 30 days after each milestone announcement, the project shall add a post-launch review to the plan recording what was announced, issues found and changes to the next milestone's criteria.

## Interfaces

- **Offers:** the launch plan; the milestone gate report; demo kits; announcement drafts.
- **Consumes:** spec tree (`generated/INDEX.md`), ECO parity reports and release candidates, OUTR.CONTENT example projects and videos, OUTR.BRAND lint, HAL simulators (via ECO releases).

## Decomposition hints

PLAN (milestone definitions), GATE (gate-check report), ANN (announcement plan), KIT (demo kits), REVIEW (post-launch reviews).

## Open questions

- Whether "1.0" of an app can precede the P2 live-device phase for that app is a maintainer choice. Opened as D-017; OUTR.LAUNCH.PLAN is blocked on it.

## Split rationale

The split follows the **launch cycle**: define milestones (PLAN), check them (GATE), announce (ANN), demonstrate at events (KIT), and learn (REVIEW). Each step has a different owner and output, and the gate sits between the plan and every public statement. The hints are kept as given.

Alternatives considered:
- **One child per milestone** (alpha, beta, 1.0). Rejected: the same plan, gate and announcement machinery serves every milestone.
- **Merge GATE into PLAN.** Rejected: the plan is written by people and blocked on D-017, while the gate is a deterministic check that should not be blocked with it.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.LAUNCH.PLAN](plan/NODE.md) | Launch plan and milestone definitions (decision D-017) | 001 002 | none |
| [OUTR.LAUNCH.GATE](gate/NODE.md) | Milestone gate check and report | 003 004 | none |
| [OUTR.LAUNCH.ANN](ann/NODE.md) | Announcement plan and drafts | 004 005 | none |
| [OUTR.LAUNCH.KIT](kit/NODE.md) | Conference and trade-show demo kits | 006 007 | none |
| [OUTR.LAUNCH.REVIEW](review/NODE.md) | Post-launch reviews | 008 | none |
