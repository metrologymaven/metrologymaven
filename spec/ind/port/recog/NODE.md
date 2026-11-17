---
id: IND.PORT.RECOG
title: Auto feature recognition from probed points
level: 3
parent: IND.PORT
kind: module
status: decision
decision: D-029
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.PORT-003]
covers: [PCD.IND.03]
depends_on: [KRN]
cli: [fid pack recognize-portable-feature]
mcp: [pack_recognize_portable_feature]
journal: [pack.recognize_portable_feature@1]
updated: 2026-11-16
---

# IND.PORT.RECOG: Auto feature recognition from probed points

## Purpose

Recognises the feature type (plane, line, circle, cylinder, sphere, point) from the points an operator took in one step, ranking candidate types so `IND.PORT.GUIDE` can compare the result with the step's expected type.

## Scope

**In:**
- `pack.recognize_portable_feature@1` (R1): candidate fits per primitive and a ranked proposal
- Use of the step's expected type only as a tie-breaker, never to hide a clearly different shape

**Out** (and which node owns it instead):
- Operator prompts and override handling (owned by `IND.PORT.GUIDE`)
- Primitive fits (owned by `KRN`)
- Possibly the inference itself, if D-029 moves it to a shared owner

## Requirements

- **REQ-IND.PORT.RECOG-001** When given the points of a step, the pack shall fit each candidate primitive the point count allows and return a ranked list of candidate types with residuals.
- **REQ-IND.PORT.RECOG-002** Recognition shall be deterministic for the same points and settings.
- **REQ-IND.PORT.RECOG-003** If no candidate's residual is below the configured limit, then the pack shall return no recognised type and the candidate list, and `IND.PORT.GUIDE` shall treat it as a mismatch.

## Interfaces

- **Offers:** `pack.recognize_portable_feature@1` (R1) returning ranked candidates.
- **Consumes:** KRN primitive fits.

## Decomposition hints

None until D-029 is decided.

## Open questions

- **Blocked on D-029:** `CMM.STRAT.LEARN` specifies the same inference (ranked candidate fits from manual hits). Whether this lives in KRN, in CMM, or here decides whether this node is a thin wrapper or a full module. Splitting it now would be guessing.

## Children

_None yet: status `decision`._
