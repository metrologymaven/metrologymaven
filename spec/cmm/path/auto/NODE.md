---
id: CMM.PATH.AUTO
title: Automatic path from a selected feature list
level: 3
parent: CMM.PATH
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.PATH-006]
covers: [PCD.PTH.05]
depends_on: [CMM.PATH.PLAN, CMM.PATH.CLEAR, CMM.COLL]
cli: [fid cmm auto-path]
mcp: [cmm_auto_path]
journal: [cmm.auto_path@1]
updated: 2026-11-16
---

# CMM.PATH.AUTO: Automatic path from a selected feature list

## Purpose

Given a list of selected features, produces a complete, collision-checked path for them: it compiles the plan, checks every move against the collision models, and where a transit or approach collides it tries deterministic remedies (raise the clearance, add a move point above the obstacle, choose another valid angle). Features it still cannot path are returned as a draft with the reason. This is the P2 "strategy-to-path generator"; auto-path from PMI is in `CMM.GEN.PMI`.

## Scope

**In:**
- End-to-end auto path for a feature list: compile, check, remedy, re-check
- A bounded, ordered set of remedies, each recorded in the result
- Draft output with an unpathed-feature list and reasons
- `cmm.auto_path@1` (W2 draft; agents write to an agent branch)

**Out** (and which node owns it instead):
- Plan compilation (owned by `CMM.PATH.PLAN`)
- Collision verdicts (owned by `CMM.COLL`)
- Feature ordering optimisation (owned by `CMM.PATH.OPT`)
- Generating features from PMI (owned by `CMM.GEN.PMI`)

## Requirements

- **REQ-CMM.PATH.AUTO-001** When given a list of features, the system shall produce a motion plan whose every move has a `CLEAR` collision verdict, or shall return each feature it could not path with the reason and the last failing verdict.
- **REQ-CMM.PATH.AUTO-002** The system shall try remedies in a fixed order (raise clearance up to the envelope limit, insert a move point above the obstacle, switch to the next valid head angle) and shall stop after a configurable number of attempts per feature (default 8).
- **REQ-CMM.PATH.AUTO-003** The system shall record every remedy applied in the result so a reviewer can see why the path differs from the default.
- **REQ-CMM.PATH.AUTO-004** The output shall be a draft that requires human adoption, and auto-path shall be deterministic (class D0) for the same inputs.
- **REQ-CMM.PATH.AUTO-005** If a collision model needed for checking is missing, then the system shall report the affected features as `UNCHECKED` and shall not treat them as pathed.

## Interfaces

- **Offers:** `cmm.auto_path@1` (W2 draft); AutoPathReport type.
- **Consumes:** `CMM.PATH.PLAN`, `CMM.PATH.CLEAR`, `CMM.PATH.ANGLE`, `CMM.COLL` verdicts.
- **Journal:** `cmm.auto_path@1`.

## Decomposition hints

LOOP (compile-check-remedy loop), REMEDY (remedy set), REPORT (draft and report).

## Open questions

- A path-planning patent freedom-to-operate review is required before P2 work starts (concept docs); that is a maintainer action, not a spec choice.

## Children

_None yet: status `open`._
