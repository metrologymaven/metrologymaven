---
id: D-036
title: Project constraints that keep grant and investment paths open
status: accepted
raised_by: founder
blocks: []
decide_by: before first public push
updated: 2026-11-17
---

# D-036: Project constraints that keep grant and investment paths open

## Question

Which project-level rules does the spec tree adopt so that the steward company stays eligible for research grants (SBIR, NSF open-source ecosystem programs) and investable, without changing D-001 or D-003?

## Context

The founder intends to fund the project through public research grants and, later, investment. Funders look for four things:
- clean IP provenance;
- an identifiable steward that owns the trademark and can receive awards;
- unproven research with measurable success criteria (not routine engineering);
- a commercialization path consistent with the open licence.

Business-formation detail stays outside this public repository, in the concept workspace. This record covers only what affects the tree. It touches ECO (contribution rules), TRUST, QUAL and OUTR (paid-service boundaries), and KRN, GDT, UNC, TRUST and AGENT (research framing).

## Options

| Option | For | Against |
|---|---|---|
| A. No funding rules in the tree | Simplest | Provenance gaps and vague research claims surface during diligence, when they are expensive to fix |
| B. Adopt the five rules below | Cheap now; makes the tree reusable directly as a proposal work plan | Slightly more frontmatter and prose discipline |

### The five rules (option B)

1. **Steward.** Until a foundation is chosen under D-003, the project is stewarded by the founder's company, which holds the trademark (D-001) and receives grants and contracts on the project's behalf. GOVERNANCE.md gets a line saying so when the repository is published.
2. **Provenance.** Every contribution is DCO signed-off (as now). A PR that contains agent-generated content says so in its description and names the agent tool. Nobody contributes material they don't have the right to license under D-001.
3. **Paid-service boundary.** D-001's paid offerings are certified builds with validation packs (QUAL), support, a hosted verifier and evidence archive (TRUST), the attestation network, and instrument-integration certification (HAL). No node may move a capability the open apps need behind a paid service. Nodes that define these services mark them `service` in their Purpose section, so the open/paid line is visible in the tree.
4. **Research framing.** The nodes that carry the project's research risk get acceptance criteria stated as **measurable hypotheses against public reference data**, not just behaviour, and get them first. The research-risk nodes:
   - KRN fitting validation;
   - GDT evaluation against reference sets;
   - UNC task-specific uncertainty;
   - TRUST independent replay;
   - AGENT adversarial safety evaluation.
5. **Disclosure hold.** Until the founder clears the conditions D-001 already lists (counsel review, the patent question) and D-002 clears the name, no node is published outside this repository, and novel method detail stays in this private tree.

## Recommendation

**B.** Rules 1–3 cost nothing now and remove diligence findings later. Rule 4 is the proposal work plan. Rule 5 restates D-001's existing conditions as a working rule for agents.

## Decision

**Accepted (option B) with amendments, 2026-11-17, founder.**

- **Rule 1 (steward):** as written; GOVERNANCE.md gets the steward line at publication.
- **Rule 2 (provenance):** as written. It **replaces** the Craft-derived "no agent attribution" rule. AGENTS.md and CONTRIBUTING.md now say: a person is accountable, and the PR names the agent tool used.
- **Rule 3 (paid-service boundary):** clarified. Validation templates (QUAL.VAP) and the IQ/OQ runner (QUAL.IQOQ) stay open in every release. The paid offering is the *certified* build with a completed validation dossier and support. Offline `fid-verify` stays free (C8); only the hosted verifier and evidence archive are paid. Service nodes mark themselves `service` in their Purpose.
- **Rule 4 (research framing):** as written. A hypothesis-framing pass runs over KRN.MAN.VAL, UNC.PROP, UNC.MODEL.SUBST, UNC.ORS, TRUST.REPLAY and AGENT.REDTEAM, and AGENTS.md requires hypothesis-style acceptance criteria on research-risk nodes.
- **Rule 5 (disclosure hold):** as written.
- **Tool-neutral pointer:** the pointer to the founder's private funding context moves into AGENTS.md (marked optional, because public contributors won't have that folder) so that every agent tool sees it, not only Claude.
