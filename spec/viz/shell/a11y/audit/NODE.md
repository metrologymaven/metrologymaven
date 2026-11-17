---
id: VIZ.SHELL.A11Y.AUDIT
title: Accessibility baseline, CI checks and release audit
level: 4
parent: VIZ.SHELL.A11Y
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-VIZ.SHELL.A11Y-005]
covers: []
depends_on: [VIZ.SHELL.A11Y.AXTREE, VIZ.SHELL.A11Y.UIHOST]
uses_artifacts: [UX.TOKENS]
side_effect: R0
determinism: D1
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# VIZ.SHELL.A11Y.AUDIT: Accessibility baseline, CI checks and release audit

## Purpose

Make the accessibility baseline explicit and checked: publish the baseline (WCAG 2.2 level AA criteria applicable to non-web software, with the list of criteria and how each is checked), run the automatable checks in CI over every panel of the test app, and run a manual audit procedure before each release whose result is published.

## Scope

**In:** `a11y-baseline.json` (format `fid.a11y-baseline`, version 1): level, criterion list with check method (automated, manual), owner leaf; CI checks over the inspection tree (role and name present, focus order, keyboard reachability of every `command` action, target size, focus-visible, contrast of rendered states against token values); release audit checklist with screen readers (NVDA on Windows, VoiceOver on macOS) and the published report.
**Out:** token contrast values (`UX.TOKENS`); theme contrast checks (`VIZ.SHELL.THEME.CHECK`); web-site checks (`OUTR.SITE.QA.A11Y`, which reads the baseline file).

## Requirements

- **REQ-VIZ.SHELL.A11Y.AUDIT-001** The project shall publish `a11y-baseline.json` naming WCAG 2.2 level AA as the target and listing each applicable success criterion with its check method and owning node [unverified: applicability set per WCAG2ICT].
- **REQ-VIZ.SHELL.A11Y.AUDIT-002** CI shall walk the inspection tree of every registered panel in the test app and fail on any node without role or name, any `command` action not reachable by keyboard, any focus trap, or any interactive target below the theme's minimum target size.
- **REQ-VIZ.SHELL.A11Y.AUDIT-003** Each release shall have an audit report covering every manual criterion of the baseline on Windows and macOS, published with the release, and a release shall not be marked accessible while any criterion is failed without a recorded exception.
- **REQ-VIZ.SHELL.A11Y.AUDIT-004** Each CI finding shall name the widget ID, the criterion and the rule.

## Acceptance criteria

- **AC-1** Given the test app with all demo panels, when the CI audit runs, then it exits 0 and reports the number of nodes checked per panel.
- **AC-2** (error path) Given a test panel with an unnamed button, then CI exits 1 naming its widget ID and criterion 4.1.2.
- **AC-3** (error path) Given a test panel whose action is pointer-only, then CI exits 1 naming criterion 2.1.1.
- **AC-4** Given the baseline file, then it validates against `schemas/a11y-baseline-1.json` and every criterion has an owner node that exists in the spec tree.

## Design

- **Registry command(s):** none; CI tool `fid-a11y-audit` and a release checklist document template.
- **Algorithm:** start test app headless with control channel; for each panel show it, take inspection tree, run rule set; keyboard reachability by simulated Tab traversal through `UIHOST`.
- **Errors:** findings; `AppStartFailed`.
- **Determinism:** D1.
- **Performance budget:** under 5 min on CI [unverified].
- **Crate:** `fid-a11y-audit` (tool) plus `docs/a11y/` templates.

## Test fixtures

Test app with demo panels and two deliberately broken test panels (feature flag).

## Build prompt

> Create tool crate `fid-a11y-audit`: start the test app, show every registered panel, read the inspection tree through the control channel, and fail on nodes without role or name (4.1.2), `command` actions not reachable by simulated keyboard traversal (2.1.1), focus traps (2.1.2), targets below the theme's minimum size (2.5.8) and insufficient focus-visible contrast using token values from the UX token package; each finding names widget ID, criterion and rule. Publish `a11y-baseline.json` (`fid.a11y-baseline` v1, schema `schemas/a11y-baseline-1.json`) and a release audit checklist for NVDA and VoiceOver. Tests AC-1..AC-4. Cite WCAG criteria by number only. Rules: CONSTITUTION §8.

## Open questions

- Applicability of EN 301 549 and the European Accessibility Act to B2B desktop software is unverified; the baseline file can add those criteria later.

## Children

_Leaf: none._
