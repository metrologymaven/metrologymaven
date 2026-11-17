---
id: OUTR.LAUNCH.REVIEW
title: Post-launch reviews
level: 3
parent: OUTR.LAUNCH
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.LAUNCH-008]
covers: []
depends_on: [OUTR.LAUNCH.PLAN]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.LAUNCH.REVIEW: Post-launch reviews

## Purpose

A short written review after each milestone announcement: what was announced, what went wrong, and what changes in the next milestone's criteria. A check makes sure the review lands within the deadline. It is separate so that lessons feed the plan through one recorded path.

## Scope

**In:**
- Review template
- Review entries added to the launch plan
- Deadline check against announcement dates
- Proposed criteria changes for the next milestone, as pull requests to the plan

**Out** (and which node owns it instead):
- The criteria themselves (owned by `OUTR.LAUNCH.PLAN`)
- Adoption numbers (owned by `OUTR.COMM.REPORT`; a review may link them)

## Requirements

- **REQ-OUTR.LAUNCH.REVIEW-001** Within 30 days after each milestone announcement, the project shall add a post-launch review to the launch plan recording what was announced, issues found, and changes proposed to the next milestone's criteria.
- **REQ-OUTR.LAUNCH.REVIEW-002** If no review exists for an announcement older than 30 days, then a scheduled check shall open or update one tracking issue naming the milestone.
- **REQ-OUTR.LAUNCH.REVIEW-003** Each review shall follow the template's sections and link every issue it mentions.
- **REQ-OUTR.LAUNCH.REVIEW-004** Every review shall pass the OUTR.BRAND claim lint with zero errors.

## Interfaces

- **Offers:** review entries; the deadline check.
- **Consumes:** OUTR.LAUNCH.PLAN milestone and announcement dates; issue tracker.

## Decomposition hints

TMPL (template), CHECK (deadline check).

## Open questions

- Becomes a leaf candidate once OUTR.LAUNCH.PLAN fixes where announcement dates are recorded.

## Children

_None yet: status `open`._
