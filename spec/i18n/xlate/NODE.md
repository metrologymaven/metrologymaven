---
id: I18N.XLATE
title: Community translation workflow, review and release gate
level: 2
parent: I18N
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-I18N-004, REQ-I18N-007]
covers: []
depends_on: [I18N.CATALOG, I18N.GLOSS, ECO.REL.PROC]
cli: [fid locale check-completeness]
mcp: [locale_check_completeness]
journal: []
updated: 2026-11-16
---

# I18N.XLATE: Community translation workflow, review and release gate

## Purpose

How translations get made, reviewed and shipped. Owns the round trip between the source catalog and the community translation platform, the per-language reviewer roles, staleness tracking when English source text changes, the completeness and review thresholds a language must meet to ship in a release, and the language roadmap. It is separate from the catalog because it is a people process with a gate, not a runtime service.

## Scope

**In:**
- Sync between the repository catalog and a translation platform (export source, import translations as pull requests, never direct pushes to `main`)
- Roles per language: translators and at least one reviewer; review state per message
- Staleness: a translation whose source digest differs from the current English source is stale and counts as missing
- Completeness metrics per language and per surface (UI, CLI, errors, report labels, docs)
- Release gate: thresholds for shipped, beta and hidden languages, enforced in the release process
- Language roadmap: German first, then Japanese and Simplified Chinese [unverified market order], then the remaining target markets
- R0 completeness report command

**Out** (and which node owns it instead):
- Catalog format, digests and argument checks (`I18N.CATALOG`)
- Terminology glossaries and the glossary check (`I18N.GLOSS`)
- Docs, tutorial and report-template localisation content (`I18N.CONTENT`, which reuses this workflow)
- Release mechanics and channels (`ECO.REL.PROC`)
- Contributor licensing terms for translations (governed by D-001 and the contribution policy)

## Requirements

- **REQ-I18N.XLATE-001** Translations shall enter the repository only as pull requests generated from the translation platform or written by contributors, and each shall pass the catalog check and the glossary check before merge.
- **REQ-I18N.XLATE-002** Each shipped language shall have at least one named reviewer, and a message shall count as complete only when it is translated, not stale and marked reviewed.
- **REQ-I18N.XLATE-003** When an English source message changes, every translation of it shall become stale until it is re-reviewed.
- **REQ-I18N.XLATE-004** A language shall ship as a selectable release language only if at least 98 % of UI, CLI and error messages and 100 % of report-template labels are complete [unverified thresholds].
- **REQ-I18N.XLATE-005** If a release candidate contains a shipped language below its threshold, then the release gate shall fail and name the language and the shortfall per surface.
- **REQ-I18N.XLATE-006** `locale.check_completeness@1` shall report, per language and per surface, translated, reviewed, stale and missing counts from the catalog alone, with no network access.
- **REQ-I18N.XLATE-007** The first release that ships a language other than English shall ship German, and the next language set shall include Japanese and Simplified Chinese [unverified market order].
- **REQ-I18N.XLATE-008** A language below the shipping threshold but above 50 % [unverified] shall be selectable only behind an explicit "incomplete translation" setting and shall be labelled as such in the UI.

## Interfaces

- **Offers:** translation sync tooling, reviewer roster per language, R0 `locale.check_completeness@1`, a release-gate check consumed by `ECO.REL.PROC`.
- **Consumes:** catalog files and digests (`I18N.CATALOG`); glossary check (`I18N.GLOSS`); release process hooks (`ECO.REL.PROC`).

## Decomposition hints

SYNC (platform round trip as pull requests), ROLES (reviewers and review states), STALE (digest-based staleness), GATE (thresholds and release check), ROADMAP (language order and its review).

## Open questions

- Which translation platform, and hosted or self-hosted. A hosted service sends source strings off-project, which is acceptable for UI text but must never carry standards text (CONSTITUTION §5) [unverified: platform terms]. Maintainers choose; if the choice binds licensing or hosting, open a decision.
- Language order beyond German needs design-partner and community input (inherited from I18N).

## Children

_None yet: status `open`._
