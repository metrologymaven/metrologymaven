---
id: SCAN.EDGE.EST.DEFS
title: Default edge method per data kind and defaults profile
level: 4
parent: SCAN.EDGE.EST
kind: task
status: leaf
phase: P1b
size: S
owners: []
claimed_by:
satisfies: [REQ-SCAN.EDGE.EST-002, REQ-SCAN.EDGE.EST-003]
covers: []
depends_on: [SCAN.EDGE.EST.TANG, SCAN.EDGE.EST.SILH, GDT.CHAR.DPROF]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# SCAN.EDGE.EST.DEFS: Default edge method per data kind and defaults profile

## Purpose

D-028 decided that both methods are offered by name and that the default is set per defaults profile, with projected silhouette as default for meshes and tangency break for one-sided sheet scans. This leaf is the single function that resolves "no method given" into a concrete method ID and parameters, and returns where the choice came from so the result can record it.

## Scope

**In:**
- Built-in default table `scan.edge.default_methods@1`: mesh actual → `projected_silhouette`; cloud actual → `tangency_break` with θ = 30°, k = 3
- Override from the characteristic's defaults profile key `scan.edge.extraction_method`, when present
- `MethodChoice { method_id, params, source: builtin | profile(ref) | explicit }`

**Out** (and which node owns it instead):
- The methods (owned by `SCAN.EDGE.EST.TANG`, `SCAN.EDGE.EST.SILH`, `SCAN.EDGE.MESHBND`)
- Profile storage (owned by `GDT.CHAR.DPROF`); the profile key schema (owned by `GDT.RES`)
- The command that records the choice (owned by `SCAN.EDGE.EDEV`)

## Requirements

- **REQ-SCAN.EDGE.EST.DEFS-001** When a method is given explicitly, the resolver shall return it unchanged with source `explicit`.
- **REQ-SCAN.EDGE.EST.DEFS-002** When no method is given and the defaults profile defines `scan.edge.extraction_method` for the actual's kind, the resolver shall return that method and parameters with source `profile` and the profile reference.
- **REQ-SCAN.EDGE.EST.DEFS-003** Otherwise the resolver shall return `projected_silhouette` for mesh actuals and `tangency_break` (θ = 30°, k = 3) for cloud actuals, with source `builtin` and table version `scan.edge.default_methods@1` [unverified defaults, pending D-028 user confirmation].
- **REQ-SCAN.EDGE.EST.DEFS-004** If the profile names an unknown method ID, then the resolver shall fail with `scan.edge.unknown_method`; the resolver shall be pure and D0.

## Acceptance criteria

- **AC-1** Given a mesh actual, no method and no profile key, the result is `projected_silhouette`, source `builtin`.
- **AC-2** Given a cloud actual and a profile setting `tangency_break` with θ = 20°, the result carries θ = 20°, source `profile` and the profile reference; given a profile naming `edge_magic`, the resolver fails with `scan.edge.unknown_method`.
- **AC-3** Given an explicit method, the result is unchanged with source `explicit`.

## Design

- **API** (`fid-scan-edge::est::defaults`): `fn resolve_method(explicit: Option<MethodSpec>, actual_kind: ActualKind, profile: Option<&DefaultsProfile>) -> Result<MethodChoice, EdgeError>`.
- **Errors:** `scan.edge.unknown_method`.
- **Determinism:** D0.
- **Crate:** `fid-scan-edge`.

## Test fixtures

Placeholder defaults profile `fixtures/scan/edge/dprof_edge_placeholder.json` (CC0).

## Build prompt

> In the Rust crate `fid-scan-edge`, module `est::defaults`, implement `resolve_method(explicit, actual_kind, profile)`: explicit wins (source `explicit`); else the defaults profile key `scan.edge.extraction_method` for the actual kind (source `profile` with the profile reference, read through the GDT.CHAR.DPROF profile object); else the built-in table `scan.edge.default_methods@1` (mesh → `projected_silhouette`, cloud → `tangency_break` θ = 30°, k = 3; source `builtin`). Unknown IDs fail `scan.edge.unknown_method`. Pure, D0. Constraints: §8, KNP-1. Tests: AC-1..AC-3.

## Open questions

- The key `scan.edge.extraction_method` must be added to the defaults-profile key schema owned by `GDT.RES`; until then only explicit and built-in sources occur.
- `SCAN.EDGE.EDEV` must call `resolve_method` when `--method` is omitted and record `MethodChoice.source`; that is an edit to EDEV for its owner.
- D-028's default table is accepted but asks for confirmation with at least two sheet-metal users [unverified].

## Children

_Leaf: none._
