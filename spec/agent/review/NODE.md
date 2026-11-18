---
id: AGENT.REVIEW
title: Independent agent reviewer
level: 2
parent: AGENT
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-007]
covers: []
depends_on: [AGENT.HARNESS]
uses_artifacts: []
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.REVIEW: Independent agent reviewer

## Purpose

Gives level-3-and-deeper changes an independent reviewer that is not the author: a different model or a fresh session with no access to the author's context. It reads the changed nodes, their ancestors and the constitution, runs `scope.py check`, checks the expand-node quality bar (MECE split, testable requirements, traceability, leaf completeness, layer-legal dependencies, no standards text), and writes a machine-readable verdict that ECO.CI can require before merge (D-037 P2).

## Scope

**In:**
- Reviewer prompt and checklist derived from prompts/expand-node.md and AGENTS.md
- Context isolation: reviewer never sees the author's transcript
- Verdict format (approve or request changes, with findings) and CI hook
- Escalation to a human for L1-L2, frozen, constitution and decision changes
- Reviewer accuracy tracking against later human overrides

**Out:**
- Enforcement of gates (CORE.BUS, CMM.EXEC); this node prepares, it never decides

## Requirements

- **REQ-AGENT.REVIEW-001** The reviewer shall run in a context that excludes the author's session and shall record a verdict that CI can require before merging a level-3-or-deeper change.

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added by D-037 (accepted 2026-11-17); not yet expanded.

## Children

_None yet: status `open`._
