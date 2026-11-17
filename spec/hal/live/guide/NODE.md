---
id: HAL.LIVE.GUIDE
title: Guided probing workflows
level: 3
parent: HAL.LIVE
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-HAL.LIVE-003]
covers: [PW.PRB.03]
depends_on: [HAL.LIVE.PROBE, KRN, VIZ]
cli: [fid device start-guided-probing, fid device skip-guided-step]
mcp: [device_start_guided_probing, device_skip_guided_step]
journal: [device.start_guided_probing@1, device.skip_guided_step@1]
updated: 2026-11-16
---

# HAL.LIVE.GUIDE: Guided probing workflows

## Purpose

Leads an operator through a plan of points and features with a portable device: prompts for the next point or feature, shows progress, checks each point for validity (distance from nominal, duplicate hits), and refuses to complete a feature until it has the minimum number of points its fit method requires. It turns a probing session into a repeatable procedure.

## Scope

**In:**
- Loading a plan of features and points (from a program or a CAD-derived plan)
- `device.start_guided_probing@1` (D1) and `device.skip_guided_step@1` (W2, with a reason)
- Next-step prompts and progress state
- Point validity checks (distance from nominal within a tolerance, duplicate detection)
- Minimum-point rule per fit method

**Out** (and which node owns it instead):
- Creating plans and programs (owned by `AUTO` and `CMM`)
- Rendering prompts (owned by `VIZ`)
- Fitting (owned by `KRN`)

## Requirements

- **REQ-HAL.LIVE.GUIDE-001** While guided probing runs, the system shall present the next planned point or feature and the count of points taken and required.
- **REQ-HAL.LIVE.GUIDE-002** The system shall refuse to complete a feature with fewer points than its fit method's minimum, returning `feature_points_insufficient` with the minimum.
- **REQ-HAL.LIVE.GUIDE-003** If a point lies farther from its nominal than the plan's tolerance, then the system shall flag it and ask the operator to retake or accept it, and the choice shall be journaled.
- **REQ-HAL.LIVE.GUIDE-004** Skipping a planned step shall require a reason and shall be journaled with it.

## Interfaces

- **Offers:** guided-probing state for `VIZ`; the commands.
- **Consumes:** `HAL.LIVE.PROBE`; `KRN` fit-method minima; plans from programs.

## Decomposition hints

PLAN (plan loading), STEP (prompt and progress state machine), CHECK (point validity), MIN (minimum-point rule; likely a leaf).

## Open questions

- The plan format is shared with CMM programs; which node owns it is a cross-domain question for `AUTO` and `CMM`.

## Children

_None yet: status `open`._
