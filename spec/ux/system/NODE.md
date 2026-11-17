---
id: UX.SYSTEM
title: Design principles, design-system package and open design workflow
level: 2
parent: UX
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-UX-001, REQ-UX-006, REQ-UX-007]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# UX.SYSTEM: Design principles, design-system package and open design workflow

## Purpose

The rules and the container for everything else in UX. It writes the design principles and the UI writing guide, defines the design-system package (format, versioning, changelog, consumption contract) that VIZ, RPT, OUTR and I18N consume, and runs the open design workflow: where design files live, how design is contributed, and the design review that every UI-changing PR passes, including the originality and trade-dress check (CONSTITUTION C9). It is separate because the package format and the review process are the shared contract all other UX children publish through; putting them in one owner keeps a single versioning and review authority.

## Scope

**In:**
- Design principles: dense but calm; data first; every number explainable; keyboard-complete; familiar workflows without incumbent trade dress (C9); status never by colour alone
- UI writing guide: voice, terminology, reserved words, units and number presentation rules in UI copy (message IDs and translation are I18N's)
- Design-system package: open, tool-neutral text format; semantic versioning; changelog with breaking/additive/fix classes; deprecation policy
- Consumption contract: consumers reference tokens, icons and components by name, never by literal value; a machine-checkable rule consumers can run in their own CI
- Design-file storage and the choice of open design tool (open question below)
- Design contribution workflow: how anyone proposes a token, component, icon, pattern or screen
- Design review in PRs that change UI, with an originality declaration and a trade-dress distinctness check
- Brand-neutral placeholders until D-002 names the product

**Out** (and which node owns it instead):
- Token values and themes (`UX.TOKENS`)
- Icon artwork (`UX.ICONS`)
- Component and pattern specs (`UX.COMP`, `UX.PATTERN`)
- Screen wireframes and mockups (`UX.SCREENS`)
- Report visual language (`UX.REPORT`)
- Usability research (`UX.RESEARCH`)
- Brand identity, logo and the trade-dress distinctness rules themselves (`OUTR.BRAND.GUIDE`; this node applies them in design review)
- Implementation of theming and token loading (`VIZ.SHELL.THEME`)

## Requirements

- **REQ-UX.SYSTEM-001** The design principles document shall define each principle listed in Scope with at least one "do" and one "don't" example, and each design review record shall cite the principles it checked.
- **REQ-UX.SYSTEM-002** The design system shall be published as a package with a semantic version, in a documented open text format that can be read and diffed without any proprietary design tool.
- **REQ-UX.SYSTEM-003** Every design-system release shall carry a changelog that classifies each change as breaking, additive or fix, and any rename or removal of a token, icon or component name shall increment the major version.
- **REQ-UX.SYSTEM-004** A deprecated token, icon or component name shall remain resolvable for at least one further minor release before removal, and the package shall list every deprecated name with its replacement.
- **REQ-UX.SYSTEM-005** The consumption contract shall define a machine-checkable rule that reports every literal colour, font size, spacing or duration value in a consumer's UI or report-style source that does not resolve to a token, so that consuming domains can fail their CI on it.
- **REQ-UX.SYSTEM-006** Every design source file referenced by a UX node shall be stored in the repository or in an open tool, and shall be retrievable and exportable to an open format without a paid account.
- **REQ-UX.SYSTEM-007** When a PR changes a token, icon, component spec, pattern, screen design or report style, or a UI implementation that consumes them, the workflow shall require a recorded design-review approval before merge.
- **REQ-UX.SYSTEM-008** Every design artefact shall carry an originality declaration (author, date, references consulted, and a statement that no incumbent code, text, icon, screenshot or layout was copied), and a design review shall not approve an artefact without one.
- **REQ-UX.SYSTEM-009** The design review checklist shall include the trade-dress distinctness rules published by the brand guidelines, and the review record shall state the result of each rule.
- **REQ-UX.SYSTEM-010** The UI writing guide shall define reserved words and their only permitted meanings, including that "approve" labels only human approval of proposals, programs and runs.
- **REQ-UX.SYSTEM-011** Until D-002 names the product, every design artefact shall use a brand-neutral placeholder name and no candidate product name.

## Interfaces

- **Offers:** the design-system package (versioned, open format) that contains the outputs of `UX.TOKENS`, `UX.ICONS`, `UX.COMP`, `UX.PATTERN` and `UX.REPORT`; the consumption-contract rule; the design-review checklist and record format; the principles and writing guide.
- **Consumes:** nothing by dependency. Interface notes (no `depends_on`, to keep UX upstream of its consumers):
  - from `OUTR.BRAND.GUIDE`: the trade-dress distinctness rules and the brand-tier token values once D-002 is decided;
  - from CORE/AUTO: the registry manifest's command title and icon fields, which name UX icons and follow the writing guide;
  - from `VIZ`, `RPT`, `OUTR`: they run the consumption-contract check in their own CI.

## Decomposition hints

PRINC (principles and writing guide), PKG (package format, versioning, changelog, deprecation), LINT (consumption-contract rule), REVIEW (design-review checklist, originality declaration, record format), TOOL (design-file storage and tool choice; may need a decision).

## Open questions

- Which open design tool and file format: Penpot (open source) or Figma with tokens exported to the repository? This needs a small maintainer decision when TOOL is expanded; it does not block this split. REQ-UX.SYSTEM-006 constrains either option.
- The token interchange format: the W3C Design Tokens Community Group format is a candidate, but its stability is [unverified]; choose at leaf level.

## Children

_None yet: status `open`._
