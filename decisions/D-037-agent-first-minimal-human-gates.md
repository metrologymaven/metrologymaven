---
id: D-037
title: Agent-first operation with a minimal fixed set of human gates
status: accepted
raised_by: founder
blocks: []
decide_by: before the next expansion
updated: 2026-11-17
---

# D-037: Agent-first operation with a minimal fixed set of human gates

## Question

Should the project, both how it is built and how the product is used, default to fully agent-driven operation, keeping human involvement only at a small, closed list of gates that law, safety or accountability require?

## Context

Founder direction (2026-11-17): focus entirely on AI and agents, and keep human involvement to the absolute minimum.

The current tree assumes humans approve along the way:
- AGENTS.md rule 8 says agents never adopt their own proposals.
- REQ-FID-021 says agents shall never perform motion, signing or controlled-record release.
- D-008 sets human approval rules for agent-authored CMM programs.
- D-009 governs DMT authorship.
- Several nodes add `human_only` commands (D-031).

Some gates cannot be removed without making the product unusable by regulated customers or unsafe:
- Physical motion can damage equipment or injure people.
- AS9100, ISO/IEC 17025 and FDA-regulated customers require a named, accountable person to release records and make conformance decisions.
- Standards text cannot be sent to external LLMs (AGENTS.md rule 5).

## Options

| Option | For | Against |
|---|---|---|
| A. Keep the current human-in-the-loop design | Already specified | Contradicts founder direction; slow; humans become the throughput limit |
| B. Fully autonomous, no human gates | Maximum speed | Regulated customers can't adopt it; motion risk; fails safety review in grant proposals |
| C. Agent-first by default, with a **closed list of human gates** that are batched, fast and audited | Nearly all the speed of B; keeps regulated markets and safety | Requires amending REQ-FID-021, D-008 and AGENTS.md rule 8 |

## Recommendation

**C.**

### Principle

Agents do everything by default. A human acts **only** at the gates listed below. Adding a gate requires a decision record. Every gate must be:
- **batched:** many items approved in one act;
- **pre-digested:** the agent presents a simulation, diff or evidence summary, never raw work;
- **fast:** one action per batch;
- **journaled.**

### Product gates (closed list)

| Gate | Human act | What agents do without a human |
|---|---|---|
| **H1 Motion envelope** | Approve a CMM or instrument program **once**, after the agent has simulated and collision-checked it, together with its motion envelope. | Author, simulate and optimize programs. **Re-execute** an approved program within its envelope on new parts, without asking again. Stop and re-request approval only if the program, fixture, machine or envelope changes. |
| **H2 Record release** | Sign or release controlled records (FAI/AS9102, calibration certificates, regulated reports), in batches. | Measure, evaluate, draft and check reports. Release non-controlled results. |
| **H3 Conformance authority** | Accept or reject parts **only where the customer's quality system requires a named person**. | Make and record decisions everywhere else, under the typed decision rules. |
| **H4 Interpretation authority** | Sign off GD&T/GPS interpretation rows (D-009) once per row. | Apply signed rows everywhere, and propose new rows with evidence. |

### Project gates (building the software)

| Gate | Human act | What agents do without a human |
|---|---|---|
| **P1 Irreversible project decisions** | Licensing, naming, publishing to a new remote, spending money, secrets, constitution changes. | Everything else. |
| **P2 Structural review** | Level 1–2 splits and `frozen` transitions. | **Level 3 and deeper:** an *independent* agent reviewer (a different model or session, without the author's context) may approve, replacing human lazy consensus. This amends AGENTS.md rule 8 for L3+. |

### Required amendments (when accepted)

- **REQ-FID-021:** reword to: "Agents shall act through the same commands as humans. Motion, controlled-record release, and named-person conformance decisions shall require the corresponding human gate (D-037 H1–H3)."
- **D-008:** replace per-motion approval with H1, a program-plus-envelope approval that can be re-executed without asking again.
- **AGENTS.md rule 8 and CONTRIBUTING.md review rules:** apply P2.
- **AGENT domain:** add requirements for the independent agent reviewer and for gate-preparation tools (simulation previews, batch release queues).
- Funding: this matches NSF pitch risk #4 (bounded autonomy for agents operating measurement equipment). See D-036 rule 4.

## Decision

**Accepted (option C) with amendments, 2026-11-17, founder.**

1. **Adoption is folded into H2.** Constitution C5 (humans adopt agent work) is satisfied at release. The H2 release batch lists every agent-authored choice the released results depend on (alignments, filters, decision rules, characteristics, form maps, explanations, program steps), pre-digested, and the human adopts and releases them in one act. Results that are never released need no adoption. The `*.adopt_*` commands remain, as items callable from the release batch; they are not separate stops.
2. **H1 keeps the physical interlocks.** H1 removes repeated *approval*, never the safety interlocks: under run-batch scope each cycle still starts only from a physical controller or pendant input, a paired hardware button, or a verified presence tap, and still begins with the reduced-speed placement check (CMM.EXEC.SCOPE-005 and -006). The setup run at reduced speed with an operator present is unchanged. D-008 is amended accordingly: agents may file run requests, but only for a program a human approved under H1, within its approved motion envelope and unchanged tuple. Released-by-other is then satisfied by construction, because the agent requests and a human approves.
3. **P2 independent agent review for level 3 and deeper.** The reviewer must be a different model or a fresh session without the author's context, must run `scope.py check`, and may approve L3+ splits and leaves. Levels 1-2, `frozen` transitions, the constitution and decisions stay human (P1). New AGENT requirements and nodes cover the reviewer (AGENT.REVIEW) and gate-preparation tools (AGENT.GATEPREP: simulation previews, batch release and approval queues).
4. **REQ-FID-021** is reworded as proposed.
