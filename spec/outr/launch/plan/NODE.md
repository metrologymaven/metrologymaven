---
id: OUTR.LAUNCH.PLAN
title: Launch plan and milestone definitions
level: 3
parent: OUTR.LAUNCH
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-001, REQ-OUTR.LAUNCH-002]
covers: []
depends_on: []
uses_artifacts: [ECO.APPS.LIST, ECO.PARITY.TRACE]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.LAUNCH.PLAN: Launch plan and milestone definitions

## Purpose

The written launch plan kept in the repository: for each focused app, what alpha, beta and 1.0 mean, and the checkable criteria for each. It also defines the machine-readable criteria format that the gate check, the roadmap page and the post-launch reviews read. It follows D-017 (accepted): an app's 1.0 needs every phase holding its core workflow finished, and earlier offline subsets ship as beta labelled by scope.

## Scope

**In:**
- The launch plan document, with one section per focused app of the D-004 family
- Milestone criteria per app and milestone, each referencing spec-tree phases, node statuses, parity numbers or a named human sign-off
- The machine-readable criteria file and milestone dates (used by OUTR.LAUNCH.GATE and OUTR.SITE.STATUS)

**Out** (and which node owns it instead):
- Phase definitions and moves (CONSTITUTION §7; decisions only)
- Evaluating the criteria (owned by `OUTR.LAUNCH.GATE`)
- Announcements (owned by `OUTR.LAUNCH.ANN`)
- Post-launch reviews (owned by `OUTR.LAUNCH.REVIEW`)

## Requirements

- **REQ-OUTR.LAUNCH.PLAN-001** The launch plan shall define alpha, beta and 1.0 for each focused app declared in the ECO.APPS manifest, following the rule chosen in D-017.
- **REQ-OUTR.LAUNCH.PLAN-002** Each milestone criterion shall be one of: a spec-node status condition, a phase condition, a parity-report threshold, or a named human sign-off role, and the criteria file shall reject any other kind.
- **REQ-OUTR.LAUNCH.PLAN-003** The criteria file shall be machine-readable, versioned in the repository, and each criterion shall have a stable ID.
- **REQ-OUTR.LAUNCH.PLAN-004** The launch plan shall record a target date for a milestone only as a milestone date entry in the criteria file, so that the roadmap page shows no other dates.
- **REQ-OUTR.LAUNCH.PLAN-005** A change to a milestone's criteria after its announcement shall be made only through a post-launch review entry or a pull request approved by a maintainer other than the author.

## Interfaces

- **Offers:** the launch plan; the criteria file format and content; milestone dates.
- **Consumes:** ECO.APPS manifest; spec-tree generated index; ECO.PARITY report fields; D-017.

## Decomposition hints

DOC (plan document and per-app criteria), FMT (criteria file format and check). APPCRIT is folded into DOC, because the criteria values are one file that one author writes.

## Open questions

- D-017 accepted (option A, offline subsets as scoped betas) and applied: OUTR.LAUNCH.PLAN.FMT enforces it with a core-phase rule table and a scope-label rule, and OUTR.LAUNCH.PLAN.DOC writes criteria under it.
- CONSTITUTION §2.1 (D-034): the former code dependency on ECO.APPS is now `uses_artifacts` on the app list file.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.LAUNCH.PLAN.FMT](fmt/NODE.md) | Milestone criteria file format and plan check (leaf) | 002 003 004 005 | none |
| [OUTR.LAUNCH.PLAN.DOC](doc/NODE.md) | Launch plan document and per-app milestone criteria (leaf) | 001 002 | none |
