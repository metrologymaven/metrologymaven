---
id: UX.RESEARCH.INQUIRY
title: Interviews and contextual inquiry before the P1a rail
level: 3
parent: UX.RESEARCH
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-UX.RESEARCH-001]
covers: []
depends_on: [UX.RESEARCH.PROTO]
uses_artifacts: [FLOW.PERSONA.PERS, FLOW.PERSONA.MAPS]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.RESEARCH.INQUIRY: Interviews and contextual inquiry before the P1a rail

## Purpose

The first round of research: at least five interviews or contextual inquiries with practising metrologists or operators who use current commercial tools, run before the P1a workflow rail design is approved. Findings are synthesised into anonymised insights linked to personas and journey steps, and published.

## Scope

**In:** the inquiry guide; session records; the synthesis `design/research/inquiry-<n>/findings.md` with an insight list (`insights@1`: id, statement, evidence count, personas, journey steps, affected spec nodes).
**Out:** protocol, consent and data rules (`UX.RESEARCH.PROTO`); usability tests (`UX.RESEARCH.USAB`); persona definitions (FLOW.PERSONA).

## Requirements

- **REQ-UX.RESEARCH.INQUIRY-001** The round shall include at least 5 participants who practise metrology or operate measuring equipment and use a current commercial tool, covering at least 3 of the FLOW personas.
- **REQ-UX.RESEARCH.INQUIRY-002** Each insight shall cite at least 2 participant codes as evidence or be marked `single-source`, and shall name the persona and journey step or spec node it affects.
- **REQ-UX.RESEARCH.INQUIRY-003** The findings shall be published after passing `fid-design research-check`, and the design review of the P1a workflow rail wireframe shall cite the findings version.

## Acceptance criteria

- **AC-1** Given the published `findings.md` and `insights.json`, when `fid-design research-check` runs, then it exits 0.
- **AC-2** Given `insights.json`, when `fid-design insights-check --root design/research/inquiry-1 --json` runs, then it reports at least 5 participant codes, at least 3 personas and no insight without evidence or `single-source`.
- **AC-3** (error path) Given fixture `fixtures/ux/research/inquiry/four-participants/`, when `insights-check` runs, then it exits 1 with `INQUIRY-PARTICIPANTS 4`.

## Design

- **Command:** `fid-design insights-check --root <dir> [--personas <file>] [--json]`. Tooling, nearest class R0.
- **Errors:** `INQUIRY-PARTICIPANTS`, `INQUIRY-PERSONAS`, `INSIGHT-EVIDENCE`, `INSIGHTS-SCHEMA`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `insights`.

## Test fixtures

`fixtures/ux/research/inquiry/four-participants/`; synthetic, CC-BY-4.0.

## Build prompt

> Run at least five interviews or contextual inquiries under the published protocol and consent rules, with practising metrologists or operators who use current commercial tools, covering at least three FLOW personas. Publish `design/research/inquiry-1/findings.md` and `insights.json` (`insights@1`: id, statement, evidence participant codes or single-source, personas, journey steps, affected spec nodes), describing incumbent workflows in your own words only. In the Rust tooling crate `fid-design`, add `fid-design insights-check --root <dir> [--personas <file>] [--json]` with errors `INQUIRY-PARTICIPANTS|INQUIRY-PERSONAS|INSIGHT-EVIDENCE|INSIGHTS-SCHEMA`. Run `fid-design research-check` before publishing. Tests AC-1..AC-3 of spec node UX.RESEARCH.INQUIRY.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; no network.

## Open questions

- Recruiting practising participants depends on design partners; feasibility is [unverified].

## Children

_Leaf: none._
