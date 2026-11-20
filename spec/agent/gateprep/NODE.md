---
id: AGENT.GATEPREP
title: Gate preparation: previews, batched queues, evidence packs
level: 2
parent: AGENT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-AGENT-008]
covers: []
depends_on: [AGENT.PROPOSE]
uses_artifacts: []
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# AGENT.GATEPREP: Gate preparation: previews, batched queues, evidence packs

## Purpose

Makes every human gate in D-037 fast: the agent prepares, the human decides once per batch. H1 gets a simulation, collision and motion-envelope preview; H2 gets a batched release queue that lists the agent-authored choices each record depends on so release and adoption are one act; H3 gets a decision summary per part; H4 gets an evidence pack per proposed DMT row.

## Scope

**In:**
- H1 preview contract (with CMM.SIM and CMM.EXEC)
- H2 batched release-and-adoption queue (with RPT.FORMS.FINAL, VIZ.INBOX and the *.adopt_* commands)
- H3 named-person decision summaries
- H4 DMT row evidence packs (with GDT.RES.ROWFLOW)
- One-action-per-batch UX contract with VIZ.INBOX

**Out:**
- Enforcement of gates (CORE.BUS, CMM.EXEC); this node prepares, it never decides

## Requirements

- **REQ-AGENT.GATEPREP-001** Each human gate H1-H4 shall be presented as one batched, pre-digested item list acted on with a single human action per batch, and every item shall link to its evidence.
- **REQ-AGENT.GATEPREP-002** The H2 release batch shall show, for every record whose `interpretation` is `provisional`, the label `interpretation: provisional` and its provisional rules, and shall offer no release action that completes while `RPT.FORMS.FINAL.CHECK` reports `provisional_label_missing` (D-045).

## Interfaces

To be refined when this node is expanded.

## Decomposition hints

Split along the Scope items above.

## Open questions

- **D-045 (accepted 2026-11-19):** REQ-AGENT.GATEPREP-002 added; its acceptance test is AC-9 of `RPT.FORMS.FINAL.CHECK`. The H4 evidence pack per row should show the content digest the reviewer signs (`GDT.RES.ROWFLOW.REVIEW`, `stale_review`).
- Added by D-037 (accepted 2026-11-17); not yet expanded.

## Children

_None yet: status `open`._
