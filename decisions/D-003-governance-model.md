---
id: D-003
title: Governance model
status: accepted
raised_by: founder
blocks: []
decide_by: before inviting outside contributors
updated: 2026-11-16
---

# D-003: Governance model

## Question

How are maintainers chosen and decisions made as the community grows?

## Context

GOVERNANCE.md proposes three stages:
- a founder-led start;
- domain maintainers listed in each level-1 node's `owners:`;
- a steering group once there are 5 maintainers.

The Craft family uses a lightweight RFC process: a maintainer starts a 3-day comment period (7 days for adding or removing an app, or changing the process), approval must come from a maintainer other than the author, every objection must say what would resolve it, and owners can override with a written reason.

## Options

| Option | For | Against |
|---|---|---|
| A. GOVERNANCE.md as written + Craft-style RFC timings | Light; proven; fits a small team | Founder-heavy at first |
| B. Foundation-hosted (e.g. Linux Foundation, NumFOCUS) from the start | Neutral; trusted by vendors | Heavy overhead before there is a community |

## Recommendation

A now, and revisit B once there are 3+ instrument vendors or 10+ maintainers.

## Decision

**Accepted (option A), 2026-11-16, founder.** GOVERNANCE.md as written, with Craft-style RFC timings: a 3-day comment period, or 7 days for adding or removing a domain or app or changing the process; approval by a maintainer other than the author; every objection must say what would resolve it. Revisit a foundation (option B) at 3+ instrument vendors or 10+ maintainers.
