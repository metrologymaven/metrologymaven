---
id: UNC.MODEL.CONTRIB.THERM
title: Thermal expansion contributor
level: 4
parent: UNC.MODEL.CONTRIB
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-UNC.MODEL.CONTRIB-003, REQ-UNC.MODEL.CONTRIB-005]
covers: []
depends_on: [UNC.MODEL.CONTRIB.CONTRACT, KRN.NUM]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# UNC.MODEL.CONTRIB.THERM: Thermal expansion contributor

## Purpose

The thermal contributor `uc:thermal@1.0.0`: the length deviation caused by part and scale temperatures differing from the 20 °C reference temperature (ISO 1), δL = L·(α_p·(t_p − 20 °C) − α_s·(t_s − 20 °C)), with the part temperature, scale temperature and both expansion coefficients as uncertain inputs. It ships as a kernel operator plus a contributor data file.

## Scope

**In:** kernel operator `unc.contrib.thermal@1.0.0` (inputs L, α_p, t_p, α_s, t_s; output δL) with analytic partial derivatives; contributor file with parameters and accepted sources (temperature: `certificate | dcc | type_a_dataset`; CTE: `declared | certificate`); valid ranges (t 10 °C to 30 °C, α 0 to 30e-6 /K, L 0 to 10 m) [unverified: default ranges].
**Out:** whether compensation is applied to the value (owned by the evaluating domain); contract and expansion (owned by `UNC.MODEL.CONTRIB.CONTRACT`).

## Requirements

- **REQ-UNC.MODEL.CONTRIB.THERM-001** The operator shall compute δL = L·(α_p·(t_p − T0) − α_s·(t_s − T0)) with T0 = 293.15 K, all quantities in SI units.
- **REQ-UNC.MODEL.CONTRIB.THERM-002** The operator shall provide analytic partial derivatives with respect to every input, agreeing with central differences at 1e-6 relative step to within 1e-8 relative.
- **REQ-UNC.MODEL.CONTRIB.THERM-003** The contributor shall treat part and scale inputs separately, so that a scale-less instrument (α_s = 0, t_s unset) is expressible by declaring the scale inputs absent.
- **REQ-UNC.MODEL.CONTRIB.THERM-004** When the expanded sub-graph is evaluated by LPU with independent inputs, the standard uncertainty of δL shall equal the root-sum-square of sensitivity times input uncertainty within 1e-12 relative.

## Acceptance criteria

- **AC-1** Given L = 0.1 m, α_p = 11.5e-6 /K, t_p = 295.15 K, α_s = 0, then δL = 2.3e-6 m within 1 ulp.
- **AC-2** Given `fixtures/unc/model/contrib/therm_cases.json` (50 seeded cases, seed `0x5448524D`), then values and derivatives match mpmath references (REQ-001, REQ-002).
- **AC-3** Given the AC-1 case with u(t_p) = 0.2 K and u(α_p) = 1e-6 /K, then the LPU result equals √((L·α_p·u_t)² + (L·Δt·u_α)²) within 1e-12 relative.
- **AC-4** (error path) Given t_p = 320 K, then `expand` fails with `out_of_range`.

## Design

- **Registry command(s):** none; kernel operator `unc.contrib.thermal@1.0.0` and data file `uc:thermal@1.0.0`.
- **Algorithm:** closed-form expression and derivatives, fixed evaluation order, no FMA.
- **Errors:** `out_of_range` (from expansion), `invalid_input` (non-finite).
- **Determinism:** D0.
- **Performance budget:** trivial.
- **Crate:** `fid-kunc` (module `contrib`), data file shipped with `fid-unc`.

## Test fixtures

`fixtures/unc/model/contrib/therm_cases.json` with mpmath references (CC-BY-4.0).

## Build prompt

> In crate `fid-kunc`, module `contrib` (kernel; KNP-1; no panic or `unwrap`), add operator `unc.contrib.thermal@1.0.0`: δL = L·(α_p·(t_p − 293.15 K) − α_s·(t_s − 293.15 K)) with analytic partials, registered in the kernel manifest. Ship contributor file `uc:thermal@1.0.0` (contract `contributor/v1`): parameters L, α_p, t_p, α_s, t_s with SI units, ranges t 283.15..303.15 K, α 0..30e-6 /K, L 0..10 m, accepted sources (temperature: certificate, dcc, type_a_dataset; CTE: declared, certificate); scale inputs optional. Tests AC-1..AC-4 with `fixtures/unc/model/contrib/therm_cases.json`.

## Open questions

- Default valid ranges are project choices and should be reviewed [unverified].

## Children

_Leaf: none._
