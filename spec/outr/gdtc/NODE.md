---
id: OUTR.GDTC
title: GD&T interpretation community
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-009]
covers: []
depends_on: [OUTR.SITE, OUTR.BRAND, OUTR.COMM]
uses_artifacts: [GDT.RES.STATUS, GDT.RES.ROWFLOW.ROSTER, UNC.ORS.TRACE]
cli: []
mcp: []
journal: []
updated: 2026-11-19
---

# OUTR.GDTC: GD&T interpretation community

## Purpose

The public side of D-045. GD&T interpretation is the part of the platform most open to dispute, and the part that only credentialed experts can settle (D-009, gate H4). This node makes that work visible and open to anyone: every interpretation rule has a public page with its owner, status and deadline; interpretation profiles are named, sponsored and capped; users submit hard cases that, once resolved, become permanent regression fixtures; experts can take a public reviewer role and are credited on the rules they sign off; and the reference suites are published so that anyone can run any software against them. Guardrails keep standards text out, keep every rule on a decision clock (D-003) and keep questions about specific customer parts out of the public forum.

**Link to the reference-data work (neutral technical terms).** Resolved cases enlarge the open reference suites (`UNC.ORS`) and feed their coverage and discrimination measures (UNC.ORS H-1 and H-4). Running published cases under two profiles (`GDT.CHAR.PDIFF`) gives a per-rule disagreement measure on public data. Both follow the measurable-hypothesis framing that AGENTS.md rule 10 sets for research-risk nodes.

## Scope

**In:**
- Public rule pages generated from the interpretation status register plus a governance register (owner, deadline, discussion link)
- The interpretation profile registry: cap, sponsors, one explicit default per dialect and edition
- The case pipeline: submission, publication, discussion, resolution, conversion into a regression fixture, credit
- The public reviewer role: qualification, conflict-of-interest declarations and recusal, credit
- The public reference suite bundle and its self-scoring tool
- Guardrails: standards-text lint over all of the above, overdue-decision escalation, routing of customer-part questions to paid support

**Out** (and which node owns it instead):
- Rule statuses and sign-off mechanics (owned by `GDT.RES.STATUS`, `GDT.RES.ROWFLOW`)
- Profile objects and the comparison engine (owned by `GDT.CHAR.DPROF`, `GDT.CHAR.PDIFF`)
- Case and suite content formats and expected-value generation (owned by `UNC.ORS`, `UNC.RDL`)
- Channels, moderation and the moderation log (owned by `OUTR.COMM`)
- The project's own benchmark result pages (owned by `OUTR.BENCH`)
- Site build and hosting (owned by `OUTR.SITE`)
- The paid support offering itself (outside this tree; listed partners in `ECO.COMM.PARTNER`)

## Requirements

- **REQ-OUTR.GDTC-001** Every rule in the interpretation status register shall have a public page showing its rule ID, version, decision owner, status (`draft`, `provisional` or `signed_off`), decision deadline, discussion link and change history.
- **REQ-OUTR.GDTC-002** Rule pages, profile pages, case pages, suite files and reviewer pages shall cite standards by designator, edition and clause number only and shall contain no reproduced standards text.
- **REQ-OUTR.GDTC-003** The project shall publish interpretation profiles only from a registry holding at most a fixed number of active profiles, each with a named sponsor, and with exactly one profile marked default per dialect and edition.
- **REQ-OUTR.GDTC-004** A submitted case (part, callout and data with a known truth from the project's generators and oracle) shall move through the states submitted, published, in discussion and resolved, and a resolved case shall become a permanent regression fixture credited to its submitter under the DCO.
- **REQ-OUTR.GDTC-005** No fixture, case page or case file shall contain personal contact data.
- **REQ-OUTR.GDTC-006** The project shall publish the public reviewer role with its qualification criteria and conflict-of-interest rules, and each rule page shall credit the reviewers who signed off its current revision.
- **REQ-OUTR.GDTC-007** The project shall publish its GD&T reference suites with inputs in open formats, expected values with tolerances and a self-scoring tool, so that anyone can run any software against them.
- **REQ-OUTR.GDTC-008** The project shall never publish, host or aggregate result comparisons against named commercial products; users run the suites and report their own results themselves.
- **REQ-OUTR.GDTC-009** Every interpretation rule shall have a decision owner and a decision deadline, and an overdue decision shall be escalated under the D-003 process.
- **REQ-OUTR.GDTC-010** A question about a specific customer part shall be routed to paid support and shall not be answered as a case or on a rule page.

## Interfaces

- **Offers:** rule pages, profile pages, case pages and the reviewer page (Markdown with front matter for `OUTR.SITE.GEN`); the governance register `gdt-rule-governance/v1`; the profile registry `gdt-profiles/v1`; the case file format `gdt-case/v1` and resolved fixtures handed to `UNC.ORS`; the reviewer register `gdt-reviewer-register/v1`; the public suite bundle and its scorer.
- **Consumes:** the status register (`GDT.RES.STATUS`), the reviewer roster (`GDT.RES.ROWFLOW.ROSTER`), the ORS case format (`UNC.ORS.TRACE`) and generators and oracle (`UNC.RDL.GEN`), the claim lint (`OUTR.BRAND.LINT`), the channel directory and moderation log (`OUTR.COMM`).

## Decomposition hints

RULES, PROFILE, CASE, REVIEW, SUITE, GUARD.

## Open questions

- The profile cap (8 active profiles) and the 12-month sponsor re-confirmation in `OUTR.GDTC.PROFILE` are proposals for maintainers to confirm by RFC under D-003.
- D-021 allows third-party comparisons published as signed statements (option B) after counsel review. For GD&T, REQ-OUTR.GDTC-008 is stricter: the project does not publish or host them at all. If D-021 option B is later used for other benchmark kinds, `OUTR.BENCH.CMP` keeps that path; this node does not.

## Split rationale

The split follows the **six community mechanisms D-045 names**, each with its own data file and its own reviewer: RULES (pages and the governance register), PROFILE (the capped registry), CASE (the case pipeline and fixtures), REVIEW (the reviewer role), SUITE (the public suite bundle and scorer) and GUARD (cross-cutting checks). Each is a small `fid-outreach` module or a policy plus a checker, so each is a leaf now.

Alternatives considered:
- **Put the reviewer role in `ECO.COMM`.** Rejected: `ECO.COMM` holds contributor tooling, while the reviewer role is a public-facing role with credit on public pages; `GDT.RES.ROWFLOW` already noted that no node owns it.
- **Put the suites in `OUTR.BENCH`.** Rejected: `OUTR.BENCH` publishes the project's own numbers, including comparisons under D-021; the GD&T suites are run by others against any software, and the project never publishes their comparisons (REQ-008).
- **Spread the guardrails over the other five children.** Rejected for the standards-text lint and the overdue escalation, which must run over every GDTC artifact the same way; the decision-owner and deadline fields themselves live in RULES.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.GDTC.RULES](rules/NODE.md) | Public rule pages and rule governance register (leaf) | 001 002 009 | none |
| [OUTR.GDTC.PROFILE](profile/NODE.md) | Interpretation profile registry: cap, sponsors and default (leaf) | 003 | none |
| [OUTR.GDTC.CASE](case/NODE.md) | Case pipeline and permanent regression fixtures (leaf) | 004 005 | none |
| [OUTR.GDTC.REVIEW](review/NODE.md) | Public reviewer role, qualification and conflicts of interest (leaf) | 006 | none |
| [OUTR.GDTC.SUITE](suite/NODE.md) | Public GD&T reference suite bundle and self-scoring (leaf) | 007 008 | none |
| [OUTR.GDTC.GUARD](guard/NODE.md) | Guardrails: standards text, overdue decisions, support routing (leaf) | 002 009 010 | none |
