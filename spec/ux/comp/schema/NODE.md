---
id: UX.COMP.SCHEMA
title: Component-spec format and completeness check
level: 3
parent: UX.COMP
kind: task
status: leaf
phase: P1a
size: M
owners: []
claimed_by:
satisfies: [REQ-UX.COMP-001, REQ-UX.COMP-002, REQ-UX.COMP-003, REQ-UX.COMP-004, REQ-UX.COMP-008]
covers: []
depends_on: [UX.SYSTEM.PKG, UX.TOKENS.COLOR, UX.TOKENS.SPACE, UX.ICONS.PROV]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UX.COMP.SCHEMA: Component-spec format and completeness check

## Purpose

The toolkit-neutral format every component spec is written in (Markdown with a structured front block) and a check that each spec is complete: anatomy, states, keyboard map, accessible role, name and states, focus indicator, token usage, density variants, disabled-reason text and reference renderings. The check is what makes REQ-UX.COMP-001 to 004 and 008 enforceable on every component the sibling nodes write.

## Scope

**In:** `component-spec@1` format; `components/TEMPLATE.md`; `fid-design comp-check`.
**Out:** the component specs themselves (`UX.COMP.DATA`, `UX.COMP.CMD`, `UX.COMP.CONT`, `UX.COMP.WHY`); reference rendering files (`UX.COMP.REF`); pattern specs (`UX.PATTERN.SCHEMA`).

## Requirements

- **REQ-UX.COMP.SCHEMA-001** The front block shall hold `id` (`comp.<name>`), `title`, `parts` (anatomy part names), `states` (from default, hover, focus, active, disabled, read-only, error, loading, each with `applies: true|false` and a reason when false), `tokens` (token names used per part and state), `density` (`compact` and `comfortable` values per part), `keyboard` (key, context, action), `a11y` (`role`, `name_source`, exposed `states`), `focus` (indicator token and geometry) and `disabled_reason` (how the reason is shown as text).
- **REQ-UX.COMP.SCHEMA-002** If any token named in a spec does not resolve in the given package, then `comp-check` shall fail and name the token.
- **REQ-UX.COMP.SCHEMA-003** If an interactive spec (one whose `parts` include an actionable part) lists a pointer action with no keyboard entry for the same action, then `comp-check` shall fail with `COMP-POINTER-ONLY`.
- **REQ-UX.COMP.SCHEMA-004** If the focus indicator token pair is not declared `non-text` in `tokens/pairs.json`, or the indicator is thinner than 2 px [unverified per WCAG 2.2 SC 2.4.13 guidance], then `comp-check` shall fail.
- **REQ-UX.COMP.SCHEMA-005** If a spec with a `disabled` state does not define `disabled_reason` as visible or focus-reachable text, then `comp-check` shall fail with `COMP-DISABLED-REASON`.
- **REQ-UX.COMP.SCHEMA-006** The `a11y.role` value shall be from a closed list of roles aligned with WAI-ARIA 1.2 role names, so that VIZ can map it to the platform accessibility API.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/ux/comp/schema/ok/button.md` and package fixture `fixtures/ux/system/pkg/v1.0.0/`, when `fid-design comp-check --root ok --package v1.0.0 --json` runs, then it exits 0.
- **AC-2** (error path) Given `pointer-only.md` (drag to reorder with no keyboard entry), when the check runs, then it exits 1 with `COMP-POINTER-ONLY reorder`.
- **AC-3** (error path) Given `bad-token.md`, when the check runs, then it exits 1 with `COMP-TOKEN-UNKNOWN sem.text.nonexistent`.
- **AC-4** (error path) Given `no-reason.md`, when the check runs, then it exits 1 with `COMP-DISABLED-REASON`.
- **AC-5** (error path) Given `bad-role.md` (`role: widgetish`), when the check runs, then it exits 1 with `COMP-ROLE`.

## Design

- **Command:** `fid-design comp-check --root <components dir> --package <dir> [--json]`. Tooling, nearest class R0.
- **Format:** Markdown file with a fenced `toml` front block followed by prose sections `## Anatomy`, `## Behaviour`, `## Keyboard`, `## Accessibility`, `## Do and don't`.
- **Errors:** `COMP-SCHEMA`, `COMP-TOKEN-UNKNOWN`, `COMP-STATE-MISSING`, `COMP-POINTER-ONLY`, `COMP-FOCUS`, `COMP-DISABLED-REASON`, `COMP-ROLE`, `COMP-DENSITY`.
- **Determinism:** D0.
- **Crate:** `fid-design`, module `comp`.

## Test fixtures

`fixtures/ux/comp/schema/`: `ok/button.md`, `pointer-only.md`, `bad-token.md`, `no-reason.md`, `bad-role.md`; synthetic, CC-BY-4.0.

## Build prompt

> Define `component-spec@1`: a Markdown file whose first block is fenced TOML with id `comp.<name>`, title, parts, states (default, hover, focus, active, disabled, read-only, error, loading; applies + reason), tokens per part and state, density (compact, comfortable), keyboard (key, context, action), a11y (role from a closed list aligned with WAI-ARIA 1.2, name_source, states), focus (token, width px, offset), disabled_reason, pointer_actions. Ship `components/TEMPLATE.md`. In the Rust tooling crate `fid-design`, add `fid-design comp-check --root <dir> --package <dir> [--json]` validating the block, token resolution through `fid-design-pkg`, keyboard coverage of pointer actions, focus pair declared non-text and at least 2 px, disabled reason present, role in list. Errors as in spec node UX.COMP.SCHEMA. Exit 0, 1, 2. Tests AC-1..AC-5.
>
> Constraints: no panic, no `unsafe`; deny `unwrap`, `expect`, `panic`, `todo`, `unimplemented`, `unreachable`; bounded parsing; no network.

## Open questions

- The focus-indicator size rule cites WCAG 2.2 SC 2.4.13 (AAA) as guidance beyond the AA rule of REQ-UX.COMP-004 [unverified that 2 px matches the SC's area formula].

## Children

_Leaf: none._
