---
id: FLOW.SETUP
title: First-run Shop Setup wizard and onboarding
level: 2
parent: FLOW
kind: subsystem
status: open
phase: P1a
size: XL
owners: []
claimed_by:
satisfies: [REQ-FLOW-001, REQ-FLOW-005]
covers: [JOB.USE.01, JOB.USE.07, JOB.USE.08]
depends_on: [TRUST.PROFILE, TRUST.SIGN, UNC.DEC, UNC.INST, RPT.MODEL, RPT.FORMS, AUTO.MCP, CORE.STO, CORE.RPL, OUTR.CONTENT, FLOW.DEADEND]
cli: [fid setup *]
mcp: [setup_show_status]
journal: [setup.*]
updated: 2026-11-16
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

- Step 2 and step 7 are A0 and step 3 is A0 per doc 10 §5; `setup.complete_step@1` is W2 only because it records wizard progress, not the setting. Confirm this does not open a side door (CONSTITUTION C3).
- Signing-identity user verification on Linux depends on D-011 (open); the step must still be completable where D-011 is undecided, perhaps marked degraded.
- Whether "defer setup" should be allowed at all under the Regulated profile is a governance question for `TRUST.PROFILE`.

## Children

_None yet: status `open`._
