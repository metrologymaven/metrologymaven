---
id: OUTR.BRAND
title: Brand, vocabulary and claim rules
level: 2
parent: OUTR
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-OUTR-007]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# OUTR.BRAND: Brand, vocabulary and claim rules

## Purpose

The rules every public-facing piece of the project must follow: brand usage guidelines (once D-002 names the project), the original visual identity, and the marketing vocabulary and claim lint that enforces CONSTITUTION C9. It is separate because every other OUTR child publishes material, and they must share one rule set and one lint, not each write their own.

## Scope

**In:**
- Brand usage guidelines: name, logo, colours, how third parties may refer to the project (after D-002)
- Original visual assets and their entries in `ATTRIBUTION.md`
- The marketing vocabulary and claim lint: rule list, allowed qualified phrasings, and the check that runs on site, docs, content and launch material
- Rules for naming incumbents: nominative use only, no trade dress, no copied text
- Rules for conformance and certification wording

**Out** (and which node owns it instead):
- Choosing the project and app names (decision D-002)
- Trademark registration and licensing terms (D-001 and the business documents outside this tree)
- Running the lint in each build (owned by `OUTR.SITE`, `OUTR.DOCS`, `OUTR.CONTENT`, `OUTR.LAUNCH`)
- The product's in-app vocabulary lint for UI strings, if any (owned by `VIZ` / `ECO`)

## Requirements

- **REQ-OUTR.BRAND-001** The project shall publish brand usage guidelines that state how the project name and logo may be used by third parties, and they shall not be published before D-002 is decided.
- **REQ-OUTR.BRAND-002** Every visual asset used in outreach material shall be original or licensed under a licence on the D-001 allowlist, and shall have an entry in `ATTRIBUTION.md`.
- **REQ-OUTR.BRAND-003** The claim lint shall flag as an error any unqualified use of "certified", "compliant", "conformant", "validated" or "traceable" (and their inflections) that is not followed in the same sentence by the standard designator, edition and the evidence link.
- **REQ-OUTR.BRAND-004** The claim lint shall flag as an error any sentence that names an incumbent product together with a superiority or equivalence claim, unless the sentence links a published OUTR.BENCH result.
- **REQ-OUTR.BRAND-005** The claim lint shall flag as an error any passage of 15 or more consecutive words that matches a registered reference text from incumbents' public material [unverified feasibility of the reference set].
- **REQ-OUTR.BRAND-006** The claim lint shall run on Markdown and HTML sources, report file, line and rule ID for every finding, and exit non-zero on any error.
- **REQ-OUTR.BRAND-007** Each lint rule shall have at least one passing and one failing fixture, and the rule list shall be versioned so that a change in rules is visible in review.
- **REQ-OUTR.BRAND-008** The brand guidelines shall forbid colours, icon styles, layouts and screenshots that imitate an incumbent's trade dress, and every outreach release shall be reviewed by a human against that rule before publication.

## Interfaces

- **Offers:** brand guidelines, asset pack, claim lint with versioned rules and fixtures, for use by all OUTR children.
- **Consumes:** D-002 (names), D-001 (licence allowlist and trademark policy), `seeds/*.md` (list of incumbent product names to detect), OUTR.BENCH (result links that justify comparisons).

## Decomposition hints

GUIDE (brand guidelines and assets, blocked on D-002), LINT (claim and vocabulary lint), REVIEW (human trade-dress review checklist).

## Open questions

- The brand guidelines part is blocked by D-002 (project name). The claim lint is not, and can proceed first.
- REQ-OUTR-007 refers to "the vocabulary lint" as if it existed. No node yet owns a product-wide vocabulary lint; this node owns the outreach one. If a product UI-string lint is wanted, it belongs to VIZ or ECO.
- Keeping a reference set of incumbent text to detect copying may itself raise copyright questions; a maintainer should confirm REQ-OUTR.BRAND-005 is wanted. Opened as D-013; OUTR.BRAND.COPY is blocked on it.

## Split rationale

The split separates **rules for people** from **rules for machines**, and isolates the parts with outside blockers. GUIDE (blocked for publication by D-002) holds the written guidelines; ASSET holds files whose provenance can be checked; LINT is the automated wording check every OUTR build runs; COPY is the copied-text rule, blocked on D-013 because it needs third-party text; REVIEW is the human trade-dress gate that automation cannot replace.

Alternatives considered:
- **Following the hints exactly** (GUIDE, LINT, REVIEW). Rejected: the copy-detection rule and the asset provenance check would have hidden blockers inside otherwise ready nodes.
- **One LINT node including copy detection.** Rejected: it would block the lint, which every other OUTR child needs, on D-013.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| [OUTR.BRAND.GUIDE](guide/NODE.md) | Brand usage guidelines and visual identity | 001 008 | none |
| [OUTR.BRAND.ASSET](asset/NODE.md) | Original outreach assets and attribution | 002 | none |
| [OUTR.BRAND.LINT](lint/NODE.md) | Outreach claim and vocabulary lint (leaf) | 003 004 006 007 | none |
| [OUTR.BRAND.COPY](copy/NODE.md) | Copied-text detection rule (decision D-013) | 005 | none |
| [OUTR.BRAND.REVIEW](review/NODE.md) | Human trade-dress review gate (leaf) | 008 | none |
