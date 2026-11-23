---
id: D-053
title: Open-core and private commercial repository boundary
status: accepted
raised_by: founder
amends: [D-001, D-036]
blocks: []
decide_by: 2026-11-22
updated: 2026-11-22
---

# D-053: Open-core and private commercial repository boundary

**Status: ACCEPTED 2026-11-22, founder.** Recorded under explicit founder direction to establish a clear, machine-enforced boundary between the public open-source platform (`metrologymaven/metrologymaven`) and the private commercial repository (`metrologymaven/metrologymaven-enterprise`) before automated build waves implement enterprise and commercial-pack domains.

## Question

Which capabilities remain in the public `MIT OR Apache-2.0` open-core repository (`metrologymaven/metrologymaven`), which capabilities move to the private proprietary commercial repository (`metrologymaven/metrologymaven-enterprise`), and how do `tools/scope.py` and CI prevent automated agents from publishing commercial implementations under `MIT OR Apache-2.0`?

## Context

Under `D-001` and `D-036`, the open platform is licensed `MIT OR Apache-2.0` (code) and `CC-BY-4.0` (specifications), and zero features needed by the four open desktop apps may be placed behind a paid licence check. At the same time, automated build waves under `D-052` are expanding and implementing leaves rapidly across the spec tree.

At the time of this decision, `ENT` (all 7 enterprise server subsystems) and all 7 first-party industry packs (`IND.AIRFOIL`, `IND.GEAR`, `IND.SHEET`, `IND.GAPFL`, `IND.TUBE`, `IND.PATTERN`, `IND.PORT`) have **zero** implemented Rust crates in this repository. Leaving them as open public build targets would publish high-value multi-site enterprise control-plane software and specialised vertical industry packs under `MIT OR Apache-2.0`, undermining the steward company's commercial sustainability and investability while adding nothing required by single-user open-source workflows.

Separating commercial implementations into a dedicated private repository preserves both commitments:
1. The public open-core platform, all four base apps, the deterministic math kernel, open formats, `fid-verify`, and the `IND.CONTRACT` plugin contract remain 100% `MIT OR Apache-2.0` with zero licence-checking or feature-gating code (`D-001`).
2. Commercial enterprise server, vertical industry packs, hosted supplier verification, fleet AI adapters, and regulated LTS products are developed and licensed commercially in `metrologymaven/metrologymaven-enterprise`, built cleanly on top of the public open-core crates and public extension points.

## Options

| Option | For | Against |
|---|---|---|
| A. Keep everything (`ENT`, `IND`, all services) in the public `MIT OR Apache-2.0` repo | Maximum code openness | Permanently gives away multi-site enterprise server and high-margin vertical packs under permissive MIT; weakens venture/OEM commercial moat |
| B. Open Core + Private Commercial Repo (`metrologymaven-enterprise`) for `ENT`, commercial first-party `IND` packs, Hosted Supplier Portal, Commercial AI/Fleet LoRA, Regulated LTS & OEM bundling | Keeps all 4 base apps, kernel, `fid-verify`, QIF/STEP/DMIS/HAL and `IND.CONTRACT` 100% open MIT/Apache-2.0 with zero paywall code; creates a clean, investable commercial product layer enforced by `scope.py` | Requires maintaining interface stubs in public `spec/` while detailed commercial leaves and code live privately |
| C. Move `LVM` (Network) and `CMM` (Probe) private as well | Wider proprietary surface | Breaks the D-004 four-open-apps promise and requires rewiring `ECO.APPS`, `FLOW`, and `MIG` |

## Recommendation

**Option B.** It honours `D-001` and `D-004` for every individual metrologist, shop, lab, and third-party plugin author while protecting the enterprise control plane, specialised industry packs, and commercial AI/hosted/regulated layers in `metrologymaven/metrologymaven-enterprise`.

## Decision

**Accepted (Option B), 2026-11-22, founder.** Amends `D-001` and `D-036` rule 3 with the following binding split and CI guardrails:

1. **Public Open-Core Repository (`metrologymaven/metrologymaven` — `MIT OR Apache-2.0` code, `CC-BY-4.0` spec):**
   - Shared platform, deterministic math kernel, geometry, QIF/STEP/DMIS/mesh/point-cloud I/O, alignment, GD&T, uncertainty, scanning, GPU viewer, reporting/SPC, CLI/MCP/Python automation, vendor-neutral `HAL`, and migration/verification tooling.
   - All four base desktop apps: **Metrology Maven Report**, **Metrology Maven Inspect**, **Metrology Maven Probe**, and **Metrology Maven Network** (`D-004`), running local-first with zero account, zero telemetry, and zero licence checks.
   - Free, open-source independent verifier (`fid-verify`, `TRUST` open core).
   - Open validation templates (`QUAL.VAP`), automated IQ/OQ runner (`QUAL.IQOQ`), per-release evidence/SBOM tooling, and open `CMM.STRAT.PROC` base process-family layer (`D-049`).
   - The public industry-pack contract and extension SDK (`IND.CONTRACT` and `ECO.SDK`), so any third party or community maintainer can build open or proprietary packs without forking.

2. **Private Commercial Repository (`metrologymaven/metrologymaven-enterprise` — Proprietary Commercial Licence, All Rights Reserved):**
   - **`ENT` (entire multi-site enterprise control plane):** `ENT.SRV`, `ENT.IAM` (SSO/SAML/LDAP/RBAC), `ENT.SYNC`, `ENT.EVENTS`, `ENT.CONNECT` (PLM/MES/ERP/QMS/SPC connectors), `ENT.DASH` (cross-site dashboards), and `ENT.OPS` (container/Kubernetes HA deployment).
   - **First-party commercial `IND` packs** (implemented strictly against public `IND.CONTRACT`): `IND.AIRFOIL`, `IND.GEAR`, `IND.SHEET`, `IND.GAPFL`, `IND.TUBE`, `IND.PATTERN`, and `IND.PORT`.
   - **Hosted Verification & Supplier Quality Verification Portal (`TRUST` commercial service layer):** hosted `fid-verify`, immutable multi-tenant evidence archive, OEM supplier package intake/mandate portal, and expert attestation network.
   - **Commercial AI, Fleet & Robotics (`CMM.STRAT.PROC` commercial tier per `D-049` and commercial AI extensions):** on-premises exact-machine / mold-cavity LoRA fine-tuning, multi-CMM fleet drift monitoring, autonomous AI inspection auto-programming, and closed-loop robotic inspection orchestration.
   - **Regulated Certified LTS & Enterprise Support (`QUAL` commercial service layer):** pre-built certified release dossiers (AS9100, IATF 16949, ISO 13485 / FDA 21 CFR Part 11, ISO/IEC 17025), frozen long-term-support branches, SLAs, and indemnification.
   - **Commercial Hardware OEM Bundling & Certified Driver Packs (`HAL` commercial service layer):** OEM co-development, certified turnkey hardware bundles, and commercial hardware integration testing.

3. **Machine-enforced repository guardrails (`tools/scope.py` and CI):**
   - Commercial-boundary spec roots in this repository declare `commercial_repo: metrologymaven-enterprise` in frontmatter: `ENT`, `IND.AIRFOIL`, `IND.GEAR`, `IND.SHEET`, `IND.GAPFL`, `IND.TUBE`, `IND.PATTERN`, and `IND.PORT`. The attribute is inherited by all descendant nodes.
   - Public `spec/ent/` and `spec/ind/<pack>/` retain their existing high-level requirement and interface stubs so parent-child traceability (`REQ-FID-028`, `REQ-FID-013`, `IND.*` workflow coverage) stays transparent and valid under `tools/scope.py check`. All detailed commercial leaves, build prompts, and implementation crates live exclusively in `metrologymaven/metrologymaven-enterprise` (using `mm-*` crate prefixes, never `fid-*`).
   - `tools/scope.py check` **fails with an error** if any node belonging to a `commercial_repo` (directly or via an ancestor) sets `built_in:` in this public repository.
   - `tools/scope.py open` **skips** all nodes belonging to a `commercial_repo` by default (unless `--include-commercial` is explicitly passed), ensuring `tools/grow.py`, `tools/grow.ps1`, and autonomous public coding agents never claim, expand, or build commercial nodes into the public `MIT OR Apache-2.0` repository.
   - `tools/scope.py milestone` flags any public milestone that pulls in a `commercial_repo` node.

4. **Non-regression of existing open commitments:**
   - No Rust crate or command definition in `metrologymaven/metrologymaven` carries licence-checking or edition-gating logic (`fid-registry` invariant preserved).
   - Previously published files in this repository remain under their existing `MIT OR Apache-2.0` / `CC-BY-4.0` licences; `D-053` governs all new commercial specifications, crates, weights, and services developed in `metrologymaven/metrologymaven-enterprise`.

## Consequences

- `tools/scope.py` adds `commercial_repo` inheritance, `built_in:` rejection for commercial nodes, and default filtering in `scope.py open`, covered by unit tests in `tools/tests/test_scope.py`.
- `spec/ent/NODE.md` and the seven commercial `spec/ind/<pack>/NODE.md` files add `commercial_repo: metrologymaven-enterprise` and point implementers to the private repository.
- `AGENTS.md`, `LICENSE.md`, `GOVERNANCE.md`, and `site/build.py` (`/company/` page) are updated to reflect the `D-053` open-core and private commercial boundary.
- Private repository `metrologymaven/metrologymaven-enterprise` is created under a proprietary commercial licence and seeded with the commercial spec tree (`ENT`, the 7 commercial `IND` packs, `PORTAL`, `AIFLEET`, `QUALCOM`, and `OEM`).
