---
id: OUTR.BRAND.GUIDE
title: Brand usage guidelines and visual identity
level: 3
parent: OUTR.BRAND
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR.BRAND-001, REQ-OUTR.BRAND-008]
covers: []
depends_on: []
uses_artifacts: [OUTR.BRAND.ASSET, OUTR.BRAND.LINT, UX.TOKENS, UX.ICONS]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# OUTR.BRAND.GUIDE: Brand usage guidelines and visual identity

## Purpose

The written brand guidelines: how the project and app names and the logo may be used by the project and by third parties, the original visual identity (colours, type, icon style, layout principles), and the rules that keep that identity clearly distinct from every incumbent's trade dress. Drafting can start now in name-agnostic form; publication waits for the name chosen under D-002.

## Scope

**In:**
- Name usage rules for the family name and the app suffixes (Inspect, Probe, Network, Report), written with a placeholder until D-002 names the family
- Logo usage rules (clear space, minimum size, forbidden alterations)
- Third-party usage rules (nominative reference, "works with" wording, no implied endorsement)
- Visual identity principles: palette, typography, icon style, screenshot conventions
- Trade-dress distinctness rules: what colours, icon styles, layouts and screenshots must not imitate

**Out** (and which node owns it instead):
- Choosing the name (decision D-002)
- Producing the asset files and their attribution (owned by `OUTR.BRAND.ASSET`)
- The human review gate that applies these rules to each release (owned by `OUTR.BRAND.REVIEW`)
- Trademark registration and licensing (D-001 and business documents outside this tree)

## Requirements

- **REQ-OUTR.BRAND.GUIDE-001** The brand guidelines shall state how third parties may refer to the project and its apps, how they may use the logo, and which uses imply endorsement and are therefore not allowed without written permission.
- **REQ-OUTR.BRAND.GUIDE-002** The guidelines shall be written with a single placeholder token for the family name, so that the name chosen under D-002 can be applied by one substitution.
- **REQ-OUTR.BRAND.GUIDE-003** If the D-002 name has not been accepted, then the guidelines shall not be published on the website or in any release.
- **REQ-OUTR.BRAND.GUIDE-004** The guidelines shall list, for each incumbent named in `seeds/*.md`, the distinctive colours, icon styles and layout traits that project material must not imitate, described in our own words [unverified per incumbent until reviewed by a maintainer].
- **REQ-OUTR.BRAND.GUIDE-005** The guidelines shall define screenshot conventions for project material: only screenshots of the project's own software, captured with its own capture commands, and never screenshots of incumbent software.

## Interfaces

- **Offers:** the brand guidelines document (draft until D-002); the trade-dress distinctness list used by OUTR.BRAND.REVIEW.
- **Consumes:** D-002 (name), D-001 (trademark exclusion), OUTR.BRAND.ASSET (logo and palette files), `seeds/*.md` (incumbent list).

## Decomposition hints

NAME (name and third-party usage), VISUAL (identity principles), DRESS (trade-dress distinctness list).

## Open questions

- Blocked for publication by D-002 (accepted as a process; the name is not yet chosen). Drafting is not blocked. OUTR.BRAND.GUIDE.NAME enforces this with a publish guard and a single `{{FAMILY_NAME}}` token.
- The former code dependency on OUTR.BRAND.ASSET is an artifact read (logo variant manifest) and is now `uses_artifacts` (CONSTITUTION 2.1, D-034). Outreach colour and type come from the shared design system (UX.TOKENS), read as artifacts.
- Describing incumbents' trade dress precisely enough to avoid it, without copying their material, needs care; a maintainer should review REQ-OUTR.BRAND.GUIDE-004 entries.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.BRAND.GUIDE.NAME](name/NODE.md) | Name, logo and third-party usage rules (leaf) | 001 002 003 | none |
| [OUTR.BRAND.GUIDE.VISUAL](visual/NODE.md) | Visual identity principles and screenshot conventions (leaf) | 005 | none |
| [OUTR.BRAND.GUIDE.DRESS](dress/NODE.md) | Trade-dress distinctness list (leaf) | 004 | none |
