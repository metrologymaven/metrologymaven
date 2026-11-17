---
id: CMM.STRAT.LIB
title: Strategy libraries and defaults
level: 3
parent: CMM.STRAT
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-009]
covers: []
depends_on: [CMM.STRAT.FEAT, CMM.STRAT.SCAN]
cli: [fid cmm *-strategy-library*, fid cmm *-strategy-default*]
mcp: [cmm_*_strategy_library*, cmm_*_strategy_default*]
journal: [cmm.*_strategy_library*@*, cmm.*_strategy_default*@*]
updated: 2026-11-16
---

# CMM.STRAT.LIB: Strategy libraries and defaults

## Purpose

Holds named, versioned strategy libraries (reusable strategy entries per feature or scan type) and the default strategy per type used when features are generated. Every feature records the library entry and version its strategy came from, so a library update is visible and traceable rather than silently changing programs.

## Scope

**In:**
- Library entity: name, version, entries keyed by feature or scan type and entry name, content digest
- Publishing a new library version; entries are immutable once published
- Default-strategy mapping per type (project-level and library-level)
- Recording `(library, version, entry)` on every feature that uses a library strategy
- Reporting features whose library entry has a newer version

**Out** (and which node owns it instead):
- Strategy types and generation (owned by `CMM.STRAT.FEAT` and `CMM.STRAT.SCAN`)
- Sharing libraries between sites or through a marketplace (owned by `ECO`)

## Requirements

- **REQ-CMM.STRAT.LIB-001** The system shall store strategy libraries as named entities with immutable, semantically versioned releases and a content digest per release.
- **REQ-CMM.STRAT.LIB-002** When a feature takes its strategy from a library, the system shall record the library ID, release version and entry name on the feature.
- **REQ-CMM.STRAT.LIB-003** The system shall provide a default strategy per feature and scan type, resolved project first, then library, and shall report which source supplied it.
- **REQ-CMM.STRAT.LIB-004** When a newer library release exists, the system shall list the features that use an older release, and shall update them only through an explicit journaled command.
- **REQ-CMM.STRAT.LIB-005** If a library entry fails its strategy type's validation, then the system shall refuse to publish the release.

## Interfaces

- **Offers:** commands such as `cmm.publish_strategy_library@1`, `cmm.set_strategy_default@1`, `cmm.update_strategy_library_usage@1` (W2), `cmm.list_strategy_library_usage@1` (R0); StrategyLibrary type.
- **Consumes:** `CMM.STRAT.FEAT` and `CMM.STRAT.SCAN` validation.
- **Journal:** `cmm.*_strategy_library*`, `cmm.*_strategy_default*`.

## Decomposition hints

STORE (library entity and releases), DEFAULT (default resolution), USAGE (usage tracking and update).

## Open questions

- Default strategies shipped with the product need a domain expert; incumbents' defaults are not copied [unverified].

## Children

_None yet: status `open`._
