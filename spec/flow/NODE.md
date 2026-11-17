---
id: FLOW
title: End-to-end workflows, personas and the acceptance suite
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-FID-025]
covers: [JOB.*]
depends_on: [CORE, AUTO, VIZ, RPT]
cli: [fid accept *, fid setup *]
mcp: [accept_*]
journal: [setup.*, accept.*]
updated: 2026-11-16
---

# FLOW: End-to-end workflows and the acceptance suite

## Purpose

Owns the **experience across domains**. Every other domain is split by capability; FLOW is split by job to be done. It defines:
- the personas;
- each end-to-end journey: which commands, screens and decisions, in what order, and where each hands off to the next;
- the first-run Shop Setup;
- the **daily-driver acceptance suite**: golden journals of real jobs, replayed nightly, with time and quality targets.

The rule: a release is not "done" because its leaves pass their tests. It is done when the jobs pass.

## Scope

**In:**
- Persona definitions and journey maps for every row in `seeds/journeys.md`
- Golden journals and data for acceptance jobs, built from public or synthetic data only
- The nightly acceptance runner, and the pass rules: gating vs tracked, median across partners
- Task-time and quality targets
- Usability bars
- The first-run Shop Setup wizard (units, governance profile, signing identity, decision rules, instrument library, templates, agent access)
- The "no dead ends" rule: every unresolved state offers a fix
- Design-partner timing-study protocol

**Out:**
- The capabilities themselves (their domains)
- Visual design and usability research methods (UX)
- Tutorials and training content (OUTR, and UX for in-app guidance)
- Incumbent correlation (MIG)

## Requirements

- **REQ-FLOW-001** Every persona and acceptance job in `seeds/journeys.md` shall have a journey map naming the commands, screens, decisions and hand-offs in order, with the owning node for each step.
- **REQ-FLOW-002** Every acceptance job shall exist as a golden journal on public or synthetic data. CI shall replay it nightly and report correctness, machine time and the outcome against its targets.
- **REQ-FLOW-003** A release shall not be marked GA for an app while any gating job of that app fails its pass rule (median across at least 3 design partners meets the target, and no partner exceeds 1.5× it).
- **REQ-FLOW-004** Every gating job shall pass unassisted, with no agent connected. Agent-assisted variants shall be measured but not gating.
- **REQ-FLOW-005** On first run, the Shop Setup wizard shall configure display units, governance profile, signing identity, default decision rules, the instrument-profile library, report templates and agent access as journaled commands. It shall not let the ASME decision-rule step be skipped silently.
- **REQ-FLOW-006** If any workflow reaches an unresolved, not-decidable or lint-failure state, then the UI and the CLI/MCP result shall offer at least one suggested fix that names a command.
- **REQ-FLOW-007** The project shall publish a design-partner timing-study protocol, and shall make no public comparative performance claim without the evidence it defines.

## Interfaces

- **Offers:** journey maps, the golden-journal acceptance suite, `fid accept run`, `fid setup`, and GA gates for ECO.REL.
- **Consumes:** every domain's commands; AUTO (programs, replay); VIZ (screens); RPT (reports).

## Decomposition hints

PERSONA (personas and journey maps), JOBS (one child per acceptance job, or grouped by app), RUNNER (nightly acceptance runner and pass rules), SETUP (Shop Setup wizard), DEADEND (suggested-fix rule and catalog), STUDY (design-partner timing protocol).

## Split rationale

The dividing principle is **by job family, aligned to the focused apps (D-004)**, with the cross-cutting machinery in its own children. Each job-family child (INSPECT, REPORT, PROBE, NETWORK) owns the maps, golden journals, data and targets of the jobs one app is accepted on, so an app's GA gate reads one child. The shared pieces each have one owner: the journey-map format and personas (PERSONA), the runner and pass rules (RUNNER), the suggested-fix rule (DEADEND), first-run setup and onboarding (SETUP) and the partner study protocol (STUDY). Job families depend on PERSONA and RUNNER; RUNNER depends on STUDY; SETUP depends on DEADEND; nothing depends back, so there are no cycles.

Alternatives considered:
- **One child per acceptance job** (10 children plus the shared ones). Rejected: over 9 children, and the jobs of one app share data generators and a gate.
- **Split by phase** (P1a jobs, P1b jobs, later jobs). Rejected: phases move (D-006, D-017); apps do not.
- **Personas as children, jobs below them.** Rejected: one job serves several personas (J1 serves the scan inspector and the quality manager), which would duplicate ownership.

## Open questions

- Should each acceptance job be its own child (10), or grouped by app? Resolved by this split: grouped by app; each job becomes a level-3 child of its family.
- Gating jobs J7 (needs `IND.GAPFL`, P3) and J10 (needs P2) cannot pass at a P1b GA; which app milestone each gates depends on D-017 (open).
- Interfaces name `fid accept run`; the children use `fid accept run-suite` to follow CONSTITUTION §3 verb-object names.
- `JOB.PER.06` and `JOB.PER.08` have no acceptance job; adding verifier and driver-SDK jobs is a seed change for maintainers.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.PERSONA](persona/NODE.md) | Personas, journey-map format and journey coverage | 001 | JOB.PER.* |
| [FLOW.SETUP](setup/NODE.md) | First-run Shop Setup wizard and onboarding | 001 005 | JOB.USE.01 JOB.USE.07 JOB.USE.08 |
| [FLOW.INSPECT](inspect/NODE.md) | Inspect-app acceptance jobs | 001 002 004 | JOB.ACC.01 JOB.ACC.05 JOB.ACC.07 |
| [FLOW.REPORT](report/NODE.md) | Report-app acceptance jobs | 001 002 004 | JOB.ACC.02 JOB.ACC.03 JOB.ACC.06 JOB.ACC.08 JOB.USE.06 |
| [FLOW.PROBE](probe/NODE.md) | Probe-app acceptance jobs | 001 002 004 | JOB.ACC.04 JOB.ACC.10 |
| [FLOW.NETWORK](network/NODE.md) | Network-app acceptance job | 001 002 004 | JOB.ACC.09 |
| [FLOW.RUNNER](runner/NODE.md) | Golden-journal acceptance runner, pass rules and GA gates | 002 003 004 | JOB.USE.04 |
| [FLOW.DEADEND](deadend/NODE.md) | No dead ends: suggested-fix catalogue and "Why?" coverage | 006 | JOB.USE.05 JOB.USE.09 |
| [FLOW.STUDY](study/NODE.md) | Design-partner timing and usability study protocol | 003 007 | JOB.USE.02 JOB.USE.03 |
