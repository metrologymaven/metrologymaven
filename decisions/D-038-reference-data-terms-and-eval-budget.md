---
id: D-038
title: Reference-data licences, external test terms and the evaluation budget
status: open
raised_by: research-hypothesis review
blocks: [UNC.RDL.EXT.NIST, UNC.RDL.EXT.PTBSG, UNC.RDL.TRACIM.PROTO, UNC.RDL.EXT.FILT, TRUST.REPLAY.CORPUS, AGENT.BOARD]
decide_by: before the first NSF full proposal
updated: 2026-11-17
---

# D-038: Reference-data licences, external test terms and the evaluation budget

## Question

The research hypotheses (generated/reviews/RESEARCH-HYPOTHESES.md) depend on outside data and paid runs that only people can arrange. Who contacts whom, and what does the project do while answers are pending?

## Context

| Item | Why it matters | Who can answer |
|---|---|---|
| NIST ATS reference data: version and redistribution terms | Fit-validation hypotheses compare against it; replay corpora may embed it | NIST (data owner) |
| PTB softgauges and TraCIM test data: redistribution, plus terms for automated or CI submissions | TraCIM is the main external validation of the fit kernel; replay corpora may embed TraCIM-derived packages | PTB (TraCIM service) |
| Filter reference data (NIST surface-metrology reference software, type F1/F2 softgauges) | Filter hypotheses have no public route without it | NIST, PTB, NPL [unverified owners] |
| JCGM guide examples | Encoded as fixtures; the guides are freely available, but the licence basis should be recorded | BIPM / JCGM |
| Funding for agent evaluation runs (red-team needs ≥ 59 runs per category per model) | AGENT.REDTEAM and AGENT.BOARD hypotheses can't be measured without it | Founder / funding plan |

## Options

| Option | For | Against |
|---|---|---|
| A. Ask each data owner in writing now; until answers arrive, store **digests only** for any external data, and use project generators for all published suites | Nothing published depends on unclear terms; work isn't blocked | Some hypotheses wait for answers |
| B. Use external data freely in private CI and decide on publishing later | Fastest | Risk of building corpora that can't be published |

## Recommendation

**A.** Concretely:
- **Data owners:** send short written requests to NIST, PTB (TraCIM) and the filter-softgauge owners covering redistribution, versions and automated submission. Record each answer as a licence-evidence record in UNC.RDL.EXT.LIC.
- **Until answers arrive:** external data stays digest-only, and published suites use project generators only, consistent with D-036 rule 5.
- **Evaluation budget:**
  - Budget model runs for AGENT.REDTEAM inside the NSF Phase I proposal, as part of the bounded-autonomy research risk.
  - Until then, run the red-team suite only on the local reference agent (AGENT.LOCAL).

## Decision

_Open._ This is a people decision. Agents must not contact external organisations.
