---
id: D-052
title: Zero-human-intervention autonomous architecture and build mode
status: accepted
raised_by: founder
amends: [D-005, D-009, D-013, D-014, D-015, D-016, D-021, D-024, D-033, D-037, D-038, D-039, D-045]
blocks: []
decide_by: 2026-11-22
updated: 2026-11-22
---

# D-052: Zero-human-intervention autonomous architecture and build mode

**Status: ACCEPTED 2026-11-22, founder.** Recorded under explicit founder direction to remove
all human build-time bottlenecks (external expert outreach, legal/counsel outreach, manual
hardware/usability gates, and human architectural review gates) while keeping clean-room
discipline, honest provisional labelling, and runtime physical machine safety intact.

## Question

How does Metrology Maven operate with **zero human intervention during specification,
architecture and implementation**, removing external expert recruitment, external data-owner
outreach, and counsel/legal outreach blockers while preserving clean-room compliance and
honest measurement provenance?

## Decision

1. **Autonomous architecture and spec decisions (amends D-037 P1/P2).**
   - Architecture, crate, solver, B-rep, UI toolkit, rendering, specification-tree (L0–L5),
     and milestone decisions are delegated to autonomous engineering, gated by independent
     agent review and CI checks (`scope.py check`, `cargo test`, `clippy`, `deny`, `layers`,
     `wasm32`, `msrv`).
   - Under this authorization, **`D-043`** (`rusqlite` bundled SQLite), **`D-049`** (PC-GNN
     process-aware CMM sampling P3 research design), **`D-050`** (Vello + Parley GPU-2D hybrid
     UI toolkit path), and **`D-051`** (GPU-driven Visibility Buffer + optional `T0_RT` tier)
     are **accepted**.
   - Only external account/credential/financial actions (secrets, DNS, spending money, changing
     licences) remain outside code commits, and none of them block specifying, building,
     testing, or shipping open-source code.

2. **Eliminate all external legal and counsel outreach blockers by choosing the clean-room,
   open-standards-only path permanently (amends D-013, D-015, D-021, D-024, D-033, D-038).**
   - **`D-038` (Accepted — self-contained Option A, zero external outreach):** Do not contact
     NIST, PTB, NPL, or BIPM or wait on external redistribution licences or paid LLM evaluation
     budgets. All verification suites (`UNC.RDL.GEN.*`, `UNC.ORS.*`) use 100% project-authored,
     clean-room mathematical generators and independent arbitrary-precision oracles. External
     data importers (`UNC.RDL.EXT.*`, `UNC.RDL.TRACIM.*`) are optional user-supplied local
     adapters only (digest/schema verification, no redistributed third-party data, excluded
     from `launch-1`). `AGENT.REDTEAM` and `AGENT.BOARD` run against deterministic scripted
     adversaries and local open-weight reference agents (`AGENT.LOCAL`).
   - **`D-024` (Amended to pure-Rust Option A/D — zero LGPL/OCCT):** Drop LGPL OCCT from the
     platform. B-rep topology, STEP parsing, and tessellation stay 100% pure Rust under
     `MIT OR Apache-2.0` (WASM-capable, `C1`-compliant, zero LGPL counsel gate).
   - **`D-015` (Amended to clean-room Option A/C permanently):** Ship only original, clean-room
     inspection and FAI report layouts plus open data exports (CSV, JSON, QIF) citing standard
     clause IDs only—never reproducing copyrighted SAE AS9102 or AIAG PPAP grid artwork, with
     no counsel gate on field ordering.
   - **`D-021` (Amended to Option A permanently):** Publish benchmarks exclusively against
     open mathematical oracles and project-generated reference suites; never publish or host
     named-incumbent comparisons (zero EULA/counsel gate).
   - **`D-033` (Amended to Option A permanently):** Support open metrology standards only
     (QIF 3.0, STEP AP242, DMIS, I++ DME, STL/PLY/OBJ/E57/LAS/CSV/PDF); permanently drop
     reverse-engineering proprietary incumbent project formats (zero EULA/counsel gate).
   - **`D-013` (Amended to Option C permanently):** Clean-room authoring only; no reference
     corpus of incumbent text and no revisit-after-counsel condition.
   - **`D-016` (Amended):** Cite standards by designator, edition, and clause number only with
     clean-room mathematical definitions; never acquire, store, paste, or distribute
     copyrighted standards text (`AGENTS.md` rule 5 remains binding).

3. **Eliminate the external expert-outreach bottleneck while keeping measurement honesty
   (amends D-009, D-039 expert queue, D-045 pacing).**
   - Remove `needs_expert:` from all open spec nodes (`GDT.*`, `UNC.*`, `HAL.IDL.CNC.*`) so
     agents can expand and build every domain without waiting on external human recruitment.
   - Lift `D-045` rule 5's hard build-pacing stop (`GDT.RES.PACE` does not block GD&T work
     outside the first-review subset).
   - **Keep `D-045`'s runtime honesty label (`interpretation: provisional`) intact:** any GD&T
     interpretation rule, uncertainty table, or protocol mapping authored without independent
     credentialed human sign-off stays visibly labelled `interpretation: provisional` (or
     `unverified`) in API results, reports, and exports. Nothing unreviewed is ever presented
     as settled.
   - Retire expert-recruitment outreach from the website, `README.md`, and `CONTRIBUTING.md`.

4. **Eliminate manual hardware and human-usability build gates (amends D-005, D-014, D-050).**
   - UI toolkit spikes (`SPKTREE`, `SPKTABLE`, `SPKRPT`, `SPKVELLO`) and performance benchmarks
     are evaluated automatically in CI (`ubuntu-latest`, `windows-latest`, `macos-latest`) using
     headless/software `wgpu` fallback and automated AccessKit tree-role assertions—never
     blocked on a dedicated human-hosted shop-floor PC or manual screen-reader runs.
   - Human-outreach, survey, moderation, usability-interview, blind-custodian, and external-NMI
     nodes are excluded from `launch-1`.

5. **Runtime physical safety preserved.**
   - Shop-floor physical CMM motion interlocks (`H1`, P2) remain intact in the product
     architecture: they protect physical machines and operators at runtime and impose zero
     human intervention on specifying, building, or testing the software.

## Consequences

- Zero open blocking decisions remain (`D-038`, `D-043`, `D-049`, `D-050`, `D-051`, `D-052`
  all accepted).
- All 43 formerly `needs_expert:` nodes enter the normal autonomous `scope.py open` queue, and
  `tools/sync_expert_issues.py` automatically closes the 43 open `expert-queue` GitHub issues
  on merge.
- `spec/CONSTITUTION.md`, `AGENTS.md`, `GOVERNANCE.md`, `CONTRIBUTING.md`, `README.md`,
  `site/build.py`, and `milestones/launch-1.md` reflect autonomous zero-human-bottleneck mode.
