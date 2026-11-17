---
id: ECO.COMM
title: Contributor workflow, RFC tooling and spec-tree tooling
level: 2
parent: ECO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-009]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.COMM: Contributor workflow and spec-tree tooling

## Purpose

The tooling that lets any person or agent find work, claim it, expand or build it, and climb the contributor ladder without maintainer help: the spec-tree tools (`scope.py`, `grow.ps1` and their successors), claim and PR-timeline bots, RFC tooling, issue templates, and the maintainer-eligibility report that implements the ladder in GOVERNANCE.md.

## Scope

**In:**
- `scope.py` (check, open, stats, generated INDEX, COVERAGE and DECISIONS) and `grow.ps1`, plus a cross-platform replacement for `grow.ps1`
- The expand-node prompt and node templates as tooling inputs
- Claim tracking: `claimed_by:`, stale-claim detection after 14 days
- Traceability from a code PR to the spec node it implements
- PR-timeline bot: 7-day response, 14-day reminder, 30-day closure, clock running only while the author is the blocker
- RFC tooling: numbered RFC files, comment-period timer (3 days, 7 for app or process changes), approval by a maintainer other than the author
- Issue and PR templates
- Maintainer-eligibility report computed from merged expansions, reviews and reverts

**Out** (and which node owns it instead):
- Governance policy and its ratification (GOVERNANCE.md, decided by D-003)
- Running these tools on PRs (owned by `ECO.CI`)
- House-rules text and sync (owned by `ECO.RULES`)
- Contributor guide pages on the docs portal (owned by `OUTR`)

## Requirements

- **REQ-ECO.COMM-001** The spec-tree check shall fail on any broken traceability (a parent requirement no child satisfies, a covered seed dropped by all children, a seed covered by no node) and shall run with only the language's standard library.
- **REQ-ECO.COMM-002** The open-node listing shall return the unclaimed open nodes at a requested level in a stable order, so parallel agents can pick work without collisions.
- **REQ-ECO.COMM-003** When a claim is older than 14 days with no linked PR, the claim bot shall mark it stale and allow takeover.
- **REQ-ECO.COMM-004** If a code PR names no spec node ID, or names one that is not a leaf, then the traceability check shall fail.
- **REQ-ECO.COMM-005** The PR-timeline bot shall post a reminder at 14 days and propose closure at 30 days of author inactivity, counting only time when the author is the blocker and ignoring bot comments.
- **REQ-ECO.COMM-006** When an RFC enters its comment period, the RFC tooling shall record the start, close it after 3 days (7 days for adding or removing an app or changing the process), and block approval by the RFC's author.
- **REQ-ECO.COMM-007** The maintainer-eligibility report shall list, per domain, contributors who meet the GOVERNANCE.md nomination thresholds, computed from repository history alone.
- **REQ-ECO.COMM-008** The spec-tree tools shall run on Windows, Linux and macOS.

## Interfaces

- **Offers:** `scope.py` (check, open, stats) and generated reports; tree-growth driver; claim, timeline and RFC bots; issue and PR templates; eligibility report.
- **Consumes:** the spec tree, seeds and decisions; repository history; GOVERNANCE.md thresholds (D-003).

## Decomposition hints

SCOPE (spec-tree validator and reports), GROW (tree-growth driver), CLAIM (claims and code-to-node traceability), BOTS (PR timeline), RFC (RFC tooling), LADDER (eligibility report).

## Open questions

- RFC timings and the ladder thresholds are proposals until D-003 is accepted; the tools should read them from config.

## Children

_None yet: status `open`._
