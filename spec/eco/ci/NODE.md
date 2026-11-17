---
id: ECO.CI
title: Shared CI actions and automated review
level: 2
parent: ECO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-005]
covers: []
depends_on: [ECO.REPO, ECO.RULES, ECO.COMM]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.CI: Shared CI and bots

## Purpose

The shared CI that every repository calls on every PR: format, lint (including the never-crash lints), tests, licence allowlist, layering check, house-rules check, spec-tree check, plus automated review bots. It owns the reusable workflow definitions and the required-check set, not the checks' internal logic, which belongs to the node that defines each rule.

## Scope

**In:**
- Reusable workflow definitions (the common-actions repo) and how a repo calls them
- The required-check set for merging to `main`
- Format and lint gates, including the denied never-crash lints and the no-`unsafe` rule
- Test execution, including crash-regression tests
- Licence allowlist and ATTRIBUTION.md presence check (SPDX list set by D-001)
- Invoking the layering check (ECO.REPO), rules check (ECO.RULES) and spec-tree check (ECO.COMM)
- Automated review bots (code review and static analysis) and how their findings are surfaced
- Rule that CI changes land in their own PRs

**Out** (and which node owns it instead):
- Logic of the layering check (owned by `ECO.REPO`)
- Logic of the rules digest check (owned by `ECO.RULES`)
- `scope.py` and the spec-tree check (owned by `ECO.COMM`)
- Release builds, signing and publishing (owned by `ECO.REL`)
- Platform test matrix and tier definitions (owned by `PLAT`)
- Reference-dataset fetching for tests (owned by `ECO.DATA`)

## Requirements

- **REQ-ECO.CI-001** Every repository shall call the shared CI workflows by an immutable reference, and merges to `main` shall require all checks in the required-check set to pass.
- **REQ-ECO.CI-002** If non-test code uses `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable` or `unsafe` outside a named exception list, then the lint gate shall fail.
- **REQ-ECO.CI-003** If a dependency's licence SPDX expression is not on the allowlist, or a third-party asset is missing from ATTRIBUTION.md, then the licence check shall fail and name the item.
- **REQ-ECO.CI-004** The shared CI shall run the layering check, the house-rules check and the spec-tree check on every PR that touches files they govern.
- **REQ-ECO.CI-005** When a PR is opened or updated, an automated reviewer shall post findings as review comments, and those findings shall be advisory unless a maintainer marks them blocking.
- **REQ-ECO.CI-006** If a PR changes CI workflow files together with any other file, then CI shall fail with a message asking for a separate PR.
- **REQ-ECO.CI-007** The shared CI shall need no secrets for checks on PRs from forks.
- **REQ-ECO.CI-008** The required-check set for a typical crate PR shall finish within 20 minutes on the project's CI runners [unverified].

## Interfaces

- **Offers:** reusable workflows; the required-check set; CI result annotations.
- **Consumes:** the layering check (ECO.REPO); the rules check (ECO.RULES); `scope.py check` (ECO.COMM); dataset fetch (ECO.DATA); D-001 licence allowlist; PLAT tier matrix for which targets to build.

## Decomposition hints

WF (reusable workflows and required checks), LINT (format and never-crash lints), LIC (licence and attribution), BOTS (review bots), GATES (calls into layering, rules and spec checks).

## Split rationale

The split is **one child per gate owned by CI**, plus the wiring. LINT and LIC are the two checks whose logic CI itself owns (REQ-ECO.CI-002 and -003); GATES decides which externally owned checks run for a change and enforces the CI-change isolation rule; WF wires everything into pinned reusable workflows and the required-check set; BOTS owns automated review. The layering, house-rules and spec-tree checks stay with ECO.REPO, ECO.RULES and ECO.COMM, as the ECO split rationale requires, so GATES only plans them.

Alternatives considered:
- **Hinted GATES as "calls into other checks" only, with isolation inside WF.** Rejected: the CI-change isolation rule is a path rule exactly like gate selection, so one planner owns both.
- **Licence allowlist content inside LIC.** Rejected: the level-2 review assigned the SPDX allowlist to ECO.RULES; LIC only evaluates against it.

## Open questions

- Which hosted review bots are acceptable given the licence decision and the rule that standards text never goes to external services (now owned by ECO.CI.BOTS) [unverified that bot configs can exclude paths].
- D-001 is accepted; the SPDX allowlist content is owned by ECO.RULES.SPDX and evaluated by ECO.CI.LIC.
- PLAT.MATRIX depends on ECO.CI, so no ECO.CI child may depend on PLAT (it would close a dependency cycle); the platform build matrix is passed into ECO.CI.WF as input data instead.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.CI.WF | Reusable workflows, required checks and test reporting | 001 007 008 | none |
| ECO.CI.GATES | Path-scoped check planning and CI-change isolation (leaf) | 004 006 | none |
| ECO.CI.LINT | Format and never-crash lint gate (leaf) | 002 | none |
| ECO.CI.LIC | Dependency licence and attribution check (leaf) | 003 | none |
| ECO.CI.BOTS | Automated review bots and finding triage | 005 | none |
