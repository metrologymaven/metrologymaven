---
id: CMM.STRAT.LEARN
title: Manually learned features
level: 3
parent: CMM.STRAT
kind: module
status: open
phase: P2
size: L
owners: []
claimed_by:
satisfies: [REQ-CMM.STRAT-007]
covers: [PCD.FEA.03]
depends_on: [KRN, CMM.STRAT.FEAT]
cli: [fid cmm learn-feature]
mcp: [cmm_learn_feature]
journal: [cmm.learn_feature@1]
updated: 2026-11-16
---

# CMM.STRAT.LEARN: Manually learned features

## Purpose

Turns hits the operator took by hand (joystick hits observed during a manual or learn-mode session) into a proposed feature type and nominal, plus a strategy that reproduces the hit layout. The result is always a draft the user must accept. It is the bridge for programmers who teach a feature on the machine instead of picking it from CAD.

## Scope

**In:**
- Feature-type inference from observed hits (candidate fits of each primitive and a ranked proposal)
- Nominal estimation for the chosen type
- Strategy extraction: hit count, levels, angles and depth that reproduce the observed layout
- Draft output and acceptance flow (`cmm.learn_feature@1`, W2 to a draft)

**Out** (and which node owns it instead):
- Capturing the manual hits on the machine (owned by `CMM.EXEC` and `HAL`; hits arrive as observations)
- Fit algorithms (owned by `KRN`)
- Strategy types (owned by `CMM.STRAT.FEAT`)

## Requirements

- **REQ-CMM.STRAT.LEARN-001** When manual hits are supplied, the system shall fit each candidate primitive allowed by the hit count and rank candidates by residual and parameter count, returning at most five candidates.
- **REQ-CMM.STRAT.LEARN-002** The system shall propose the top-ranked candidate's type and nominal and a strategy whose generated hit targets reproduce the observed hits within a configurable tolerance (default 0.5 mm [unverified]).
- **REQ-CMM.STRAT.LEARN-003** The system shall store the proposal as a draft feature flagged `learned`, which takes effect only after a user accepts it in a separate journaled command.
- **REQ-CMM.STRAT.LEARN-004** If no candidate's residual is below the configured limit, then the system shall return `LEARN_NO_CANDIDATE` with the candidate list and create no draft.
- **REQ-CMM.STRAT.LEARN-005** Learning shall be deterministic for the same hits and settings (class D0 or the kernel fit class).

## Interfaces

- **Offers:** `cmm.learn_feature@1` (W2 draft), acceptance through `cmm.set_strategy@1`; LearnedFeatureProposal type.
- **Consumes:** `KRN` primitive fits; `CMM.STRAT.FEAT` strategy types and generators.
- **Journal:** `cmm.learn_feature@1`.

## Decomposition hints

INFER (type inference and ranking), EXTRACT (strategy extraction), DRAFT (draft and acceptance).

## Open questions

- Ranking criteria between, for example, a short cylinder and a circle need a domain expert.

## Children

_None yet: status `open`._
