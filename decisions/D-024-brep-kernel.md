---
id: D-024
title: B-rep kernel - own Rust crate or OCCT out of process
status: open
raised_by: GEO.CAD split
blocks: [GEO.CAD.TESS, IO.CAD]
decide_by: before GEO.CAD.TESS or the IO.CAD STEP geometry reader is expanded
updated: 2026-11-16
---

# D-024: B-rep kernel - own Rust crate or OCCT out of process

## Question

Which engine builds, validates and tessellates B-rep bodies: a pure-Rust B-rep crate written (or adopted) by the project, or Open CASCADE Technology (OCCT, LGPL-2.1 with an exception [unverified exact licence terms]) running in a separate worker process?

## Context

- The design baseline assumed OCCT out of process. GEO (level 1) and GEO.CAD left the choice open; IO.CAD (REQ-IO.CAD-010) already requires CAD readers to run isolated from the document process.
- C1 requires every value of record to come from the pure-Rust, f64, deterministic kernel. Point-to-face projection (GEO.DIST.FACE), trimmed-face classification (GEO.CAD.TOPO) and nominal recognition (GEO.CAD.NOMINAL) produce values of record, so whichever option is chosen, those operations must run in Rust on the Rust topology model. The real choice is about **tessellation, healing and building B-rep from STEP/IGES**.
- Tessellation must be D1 per kernel version (REQ-GEO.CAD-005) and watertight (REQ-GEO.CAD-004). The WASM web build (CONSTITUTION §2) cannot run a native C++ worker.
- D-001 sets MIT OR Apache-2.0 for project code; the dependency SPDX allowlist is still to be written in ECO.RULES. Whether an LGPL component in a separate process is acceptable is a licensing call for maintainers and counsel.
- Effort: robust trimmed-surface tessellation and STEP B-rep construction in Rust is a large, specialist job [unverified estimate: several person-months]; OCCT gives near-term parity but brings a C++ toolchain on three OSes.
- D-019 (native CAD formats) is related: translator plugins may themselves wrap OCCT or commercial kernels.

Blocks: GEO.CAD.TESS (tessellation engine) and IO.CAD (how STEP geometry becomes GEO bodies). GEO.CAD.TOPO, GEO.CAD.NOMINAL, GEO.CAD.GROUP and GEO.CAD.ASSY are not blocked: they work on the Rust topology model under every option.

## Options

| Option | For | Against |
|---|---|---|
| A. Own pure-Rust B-rep crate (topology, validation, STEP B-rep building in IO, tessellation in `fid-geo-cad`) | Fully within C1; licence-clean; runs in WASM; one language; deterministic by construction | Largest effort; slower to parity on messy real-world STEP files; no healing at first |
| B. OCCT in a worker process for STEP/IGES building, healing and tessellation; results converted into the Rust topology model | Mature; fast parity; healing available | LGPL in the distribution; C++ build on three OSes; no WASM tessellation (web needs stored tessellations); determinism tied to the pinned OCCT build, likely only D1 per platform or N |
| C. Hybrid: Rust owns the topology model, tessellation and all value-of-record operations; OCCT is an optional, separately installed translator/healer plugin (A0) used only at import, never on the value-of-record path | Core stays C1-clean and WASM-capable; users who need healing get it; OCCT also usable as an offline test oracle | Still needs the Rust tessellator; two import paths to test; plugin packaging work |
| D. Adopt an existing Rust B-rep crate (for example `truck`, Apache-2.0 [unverified licence and maturity]) as the base of option A | Less code to write; licence fits D-001 | Maturity and STEP coverage unknown [unverified]; KNP-1 compliance (no FMA, no HashMap, `fid-num` transcendentals) would need a fork |

## Recommendation

C, with D evaluated as a starting point for the Rust tessellator in a time-boxed spike. Values of record stay in Rust under every option; the decision mainly sets who builds tessellations and STEP B-rep. Get counsel's view on shipping or recommending an LGPL plugin before choosing B or C.

## Decision

_Open._ When decided: update GEO.CAD.TESS (status open, scope fixed to the chosen engine), IO.CAD (STEP geometry reader boundary) and, if B or C, ECO.RULES (allowlist entry and plugin packaging).
