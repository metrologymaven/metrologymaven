---
id: FLOW.SETUP
title: First-run Shop Setup wizard and onboarding
level: 2
parent: FLOW
kind: subsystem
status: split
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-005]
covers: [JOB.USE.01, JOB.USE.07, JOB.USE.08]
depends_on: [TRUST.PROFILE, TRUST.SIGN, UNC.DEC, UNC.INST, RPT.MODEL, RPT.OUTPUT, CORE.STO, CORE.RPL, CORE.PRJ, CORE.BUS, ECO.DATA]
uses_artifacts: [OUTR.CONTENT, FLOW.DEADEND, FLOW.PERSONA, FLOW.RUNNER, FLOW.STUDY]
cli: [fid setup *]
mcp: [setup_*]
journal: [setup.*]
updated: 2026-11-17
---

# FLOW.SETUP: First-run Shop Setup wizard and onboarding

## Purpose

Owns the first-run experience: the Shop Setup wizard that takes a fresh install to a configured shop, the sample job on public data, and the onboarding path into the first tutorial. The wizard is a **sequencer**: each step invokes the owning domain's registry command (governance profile from TRUST, decision rules from UNC, instrument profiles from UNC, templates from RPT, agent access from AUTO), so every setting is journaled exactly as if it were made later from the normal UI. SETUP adds only wizard state and the rule that the ASME decision-rule step cannot be skipped silently.

## Scope

**In:**
- Wizard sequence, its eight steps and their order: (1) organisation, site, display units and date format; (2) governance profile; (3) signing identity; (4) default decision rule per dialect; (5) instrument-profile library; (6) report templates; (7) agent access; (8) sample job
- Wizard state: started, per-step done or acknowledged-skip, completed; resumable after interruption
- The explicit, journaled acknowledgement required to leave step 4 without an ASME rule, and the link back to it from `rule_missing` results
- The sample job on public data, and the hand-off into the first tutorial (step mode with "your turn" checks)
- The time-to-first-project target and its measurement (`JOB.USE.01`)
- The setup journey map (in the `FLOW.PERSONA` format)

**Out** (and which node owns it instead):
- The commands each step invokes: governance profile (`TRUST.PROFILE`), signing identity enrolment (`TRUST.SIGN`), decision rules (`UNC.DEC`), instrument profiles (`UNC.INST`), report templates (`RPT.MODEL`, `RPT.FORMS`), agent roots and MCP allowlist (`AUTO.MCP`), settings storage (`CORE.STO`)
- Tutorial content and its example projects (owned by `OUTR.CONTENT`)
- The step-mode replay mechanism (owned by `CORE.RPL`)
- The suggested-fix catalogue entry for `rule_missing` (owned by `FLOW.DEADEND`)
- Visual design of the wizard (owned by `UX`)

## Requirements

- **REQ-FLOW.SETUP-001** When the app starts with no completed Shop Setup in the local configuration, the system shall offer the Shop Setup wizard before any project can be created, and shall allow the user to defer it only by an explicit journaled choice.
- **REQ-FLOW.SETUP-002** Each wizard step shall make its change only by invoking the owning domain's registry command, so that the journal records the same command IDs as a change made outside the wizard.
- **REQ-FLOW.SETUP-003** The wizard shall present steps 1 to 8 in the order listed in Scope, and shall show each step's default (display units and date format from the OS locale; Lean profile; agent access off; export-control tagged data default-deny; no ASME rule).
- **REQ-FLOW.SETUP-004** If the user leaves step 4 without choosing an ASME default decision rule, then the wizard shall require a journaled acknowledgement (`setup.acknowledge_skip@1`) stating that ASME results needing a rule will be NOT_DECIDABLE with reason `rule_missing`.
- **REQ-FLOW.SETUP-005** When a result is NOT_DECIDABLE with reason `rule_missing`, its suggested fix shall open wizard step 4 or name the equivalent `UNC.DEC` command.
- **REQ-FLOW.SETUP-006** The wizard shall not expose steps 2, 3, 4, 5, 6 or 7 to an agent principal; over MCP only the read-only `setup_show_status` shall be available, and an agent may at most propose a template draft that a human adopts (CONSTITUTION C5).
- **REQ-FLOW.SETUP-007** If the wizard is interrupted (crash, kill or close) after any step, then on next start it shall resume at the first step not done or acknowledged, with completed steps' journaled settings intact.
- **REQ-FLOW.SETUP-008** The sample job (step 8) shall use only public or synthetic data shipped with or fetched by digest from the dataset catalogue, and shall reach an exported report without network access once the data is present.
- **REQ-FLOW.SETUP-009** After the sample job, the system shall offer the first tutorial of the user's primary app, replayed in step mode, where each "your turn" gate checks the resulting entity state deterministically before advancing.
- **REQ-FLOW.SETUP-010** The setup journey shall reach a created first project in under 10 minutes under the Lean profile and under 20 minutes under the Regulated profile for the median design partner [unverified].
- **REQ-FLOW.SETUP-011** The setup journey shall have a journey map in the `FLOW.PERSONA` format, and a golden journal of a full Lean-profile setup replayed by `FLOW.RUNNER`.

## Interfaces

- **Offers:** `setup.start_wizard@1` (W2), `setup.show_status@1` (R0), `setup.complete_step@1` (W2), `setup.acknowledge_skip@1` (W2, human only), `setup.run_sample_job@1` (W2). CLI `fid setup *`; journal `setup.*`.
- **Consumes:** the step commands of `TRUST.PROFILE`, `TRUST.SIGN`, `UNC.DEC`, `UNC.INST`, `RPT.MODEL`, `RPT.FORMS`, `AUTO.MCP`; settings from `CORE.STO`; step mode from `CORE.RPL`; tutorials from `OUTR.CONTENT`; fix entries from `FLOW.DEADEND`.

## Decomposition hints

WIZARD (state machine, resume, defaults), STEPS (binding of each step to its owning command, one feature per step group), RULEGATE (skip acknowledgement and `rule_missing` link), SAMPLE (sample job and data), ONBOARD (tutorial hand-off and "your turn" checks), TIME (time-to-first-project measurement).

## Open questions

- Resolved (side door): `setup.complete_step@1` carries no setting value and is accepted only when the journal shows the step's bound domain command committed since wizard start, or an allowed default is kept (`FLOW.SETUP.STEPS`). Settings are made only by their owning commands, which keep their own class (A0 for steps 2 and 7).
- Resolved by D-011 (accepted): Linux signing uses a hardware token where present, otherwise a passphrase, with the method recorded; Regulated requires the token. Step 3 is completable on Linux.
- Whether "defer setup" should be allowed at all under the Regulated profile is a governance question for `TRUST.PROFILE`; `FLOW.SETUP.WIZARD` allows it for every profile until told otherwise.
- The tutorial "your turn" check (`setup_check_turn`, R1) is on MCP; REQ-006's MCP limit applies to the wizard steps only. Human-only wizard commands appear on MCP as generated request-only tools (CONSTITUTION §3, D-031), so `setup_show_status` stays the only directly callable wizard tool.

## Split rationale

The dividing principle is **the wizard's layers**: what each step binds to (STEPS, a pure table and predicate), the state machine and its commands (WIZARD), the one guarded step (RULEGATE), the sample job (SAMPLE), the tutorial hand-off (ONBOARD), and the journey as an acceptance artifact with its time target (JOURNEY). WIZARD depends on STEPS; RULEGATE and SAMPLE depend on WIZARD; JOURNEY depends on the three; nothing depends back.

Alternatives considered:
- **One child per wizard step.** Rejected: steps 1, 5, 6 and 7 are a few table rows each; only step 4 has its own behaviour.
- **Time measurement as its own child.** Rejected: it is a target in the journey packages plus a study task definition; the measurement method belongs to `FLOW.STUDY`.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [FLOW.SETUP.STEPS](steps/NODE.md) | Wizard step bindings to owning domain commands | 002 003 006 | JOB.USE.07 |
| [FLOW.SETUP.WIZARD](wizard/NODE.md) | Wizard state machine, commands and resume | 001 003 006 007 | JOB.USE.07 |
| [FLOW.SETUP.RULEGATE](rulegate/NODE.md) | Decision-rule skip acknowledgement and rule_missing link | 004 005 | JOB.USE.07 |
| [FLOW.SETUP.SAMPLE](sample/NODE.md) | Sample job on public data (wizard step 8) | 008 | JOB.USE.07 |
| [FLOW.SETUP.ONBOARD](onboard/NODE.md) | First tutorial hand-off in step mode with "your turn" checks | 009 | JOB.USE.08 |
| [FLOW.SETUP.JOURNEY](journey/NODE.md) | Setup journey map, golden setup journal and time-to-first-project target | 010 011 | JOB.USE.01 |
