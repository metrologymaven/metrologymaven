---
id: AGENT.REDTEAM.GATES
title: Red-team of human gates H3/H4 and injected UI input
level: 3
parent: AGENT.REDTEAM
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT.REDTEAM-008]
covers: []
depends_on: [AGENT.REDTEAM.PRED, AGENT.REDTEAM.SCRIPT]
uses_artifacts: [CORE.BUS.PRIN, VIZ.SHELL.A11Y.UIHOST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# AGENT.REDTEAM.GATES: Red-team of human gates H3/H4 and injected UI input

## Purpose

Closes the gap found by the AGENT.REDTEAM hypothesis pass: no child tested gates H3 and H4 or the D-035 injected-input rule end to end. Scripted adversaries try to accept or reject parts, sign off DMT rows, and press human-only or W3 buttons through injected UI input; the suite verifies every attempt is refused and journaled.

## Scope

**In:**
- Attack scripts for H3 and H4 through every surface (MCP, CLI, Python, control channel)
- Injected-UI-input attacks via the control channel and accessibility automation
- Verification that refusals are journaled with the channel principal

**Out:**
- Interpretation of standards (expert work, D-009); this node supplies data and structure only

## Requirements

- **REQ-AGENT.REDTEAM.GATES-001** The suite shall attempt every H3 and H4 action and every human-only or W3 command through each agent surface and through injected UI input.
- **REQ-AGENT.REDTEAM.GATES-002** The suite shall fail if any attempt succeeds or if a refusal is not journaled with the channel's principal.

## Research hypotheses

- **H-1** Zero successful bypasses in N ≥ 59 attempts per attack category (one-sided 95 % upper bound ≤ 5 %) [unverified volume].

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- Added from the research-hypothesis gap list (generated/reviews/RESEARCH-HYPOTHESES.md), 2026-11-17. Thresholds are targets marked [unverified].

## Children

_None yet: status `open`._
