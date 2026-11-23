---
id: FID
title: Open dimensional metrology platform
level: 0
parent:
kind: product
status: split
phase: P0
size: XXL
owners: [founder]
claimed_by:
satisfies: []
covers: []
depends_on: []
cli: [fid *]
mcp: [*]
journal: [*]
updated: 2026-11-22
---

# FID: Open dimensional metrology platform

## Purpose

An open-source, QIF-native, MCP- and CLI-first, GPU-rendered, fully recordable dimensional metrology platform. It provides four focused apps on one shared core for reporting and SPC, scan-to-CAD inspection, open-standard CMM programming and execution (I++ DME and DMIS), and multi-instrument large-volume networks. It is built by a self-sustaining community of people and coding agents.

## Scope

**In:**
- Scan-to-CAD inspection
- Prismatic and freeform GD&T evaluation
- Uncertainty and decision rules
- CMM programming, simulation and execution through I++
- Portable and large-volume instrument networks
- Reporting and SPC
- Industry analysis packs
- The automation surfaces (CLI, MCP, Python, control channel, recording)
- The community and extension machinery

**Out:**
- Scanner and CMM hardware and firmware
- CT volume analysis (deferred)
- Strain and DIC analysis
- Reverse-engineering surface modelling

See `seeds/out-of-scope.md`.

## Requirements

- **REQ-FID-001** The system shall support complete scan-to-CAD inspection: import scan data and CAD, align, compare surfaces, evaluate dimensions and GD&T, and report.
- **REQ-FID-002** The system shall program, simulate and execute tactile and non-contact CMM measurements on any controller that implements I++ DME, and every motion shall be approved by a human.
- **REQ-FID-003** The system shall combine measurements from several portable and large-volume instruments into one adjusted network with stated uncertainty.
- **REQ-FID-004** Every capability shall be a registry command with a generated CLI command, MCP tool, Python binding and documentation entry. A feature without these shall not ship.
- **REQ-FID-005** Every session shall be recordable as a semantic journal that can be replayed, diffed and turned into a reusable program. Screenshots and video capture shall be commands.
- **REQ-FID-006** Values of record shall be computed only by the deterministic CPU kernel. GPU computation shall be for display only.
- **REQ-FID-007** Feature fitting shall offer least-squares, minimum-zone, minimum-circumscribed, maximum-inscribed and tangential methods, validated against published reference data.
- **REQ-FID-008** Evaluation shall cover dimensional and GD&T characteristics under ASME Y14.5-2018 and ISO GPS, with the dialect chosen explicitly for each characteristic.
- **REQ-FID-009** Every result shall carry its uncertainty status and a typed conformance decision rule.
- **REQ-FID-010** Data exchange shall be QIF-native. The system shall support standard mesh, point cloud, CAD (STEP AP242 with PMI), DMIS and SPC formats.
- **REQ-FID-011** Instruments shall be accessed through a vendor-neutral hardware abstraction, with a simulator for every device class.
- **REQ-FID-012** The system shall produce inspection reports and multi-part statistics (trend, SPC, capability).
- **REQ-FID-013** Industry-specific analyses (airfoil, gear, sheet metal, gap and flush, tube) shall ship as installable packs built on public extension points.
- **REQ-FID-014** Third parties shall be able to extend the system through a stable SDK and plugin interface, without forking.
- **REQ-FID-015** The project shall be self-sustaining. Every unit of work shall be a claimable spec node validated by CI, and there shall be a documented path from first contribution to maintainer.
- **REQ-FID-016** The GUI shall render scan data at interactive rates on commodity GPUs.
- **REQ-FID-017** The system shall run local-first on Windows, Linux and macOS, with no account and no telemetry unless the user opts in.
- **REQ-FID-018** Any exported result shall be independently verifiable with a free, open-source verifier.
- **REQ-FID-019** Alignments (best-fit, local, 3-2-1, RPS, datum-based, constrained, chained) shall be first-class, journaled and re-runnable objects.
- **REQ-FID-020** The geometry model shall hold points, meshes, point clouds and exact CAD (B-rep and NURBS) together, with consistent units and coordinate frames.
- **REQ-FID-021** Agents shall act through the same commands as humans. Motion, controlled-record release, and named-person conformance decisions shall require the corresponding human gate (D-037 H1–H3).
- **REQ-FID-022** The project shall have a public website, a docs portal and an adoption plan whose claims are generated from, or checked against, the spec tree and parity reports.
- **REQ-FID-023** The project shall publish and test a support matrix of operating systems, architectures, GPU backends and hardware profiles, including offline shop-floor deployment.
- **REQ-FID-024** The system shall support a published catalog of instruments and data sources (see `seeds/devices.md`), each through a named integration path and phase.
- **REQ-FID-025** Each app shall be accepted against end-to-end jobs and personas (`seeds/journeys.md`) with time, quality and usability targets, replayed nightly from golden journals.
- **REQ-FID-026** Users of incumbent software shall be able to migrate in stages, and shall be able to compare our results with their incumbent's on the same data, with every significant difference explained.
- **REQ-FID-027** Regulated and accredited customers shall receive validation evidence packs. The project shall operate a documented security, vulnerability-handling and product-compliance process.
- **REQ-FID-028** Multi-user and multi-site organisations shall be able to self-host a server for results, programs and identities, and to integrate with MES, QMS, ERP, PLM and SPC systems, without losing local-first operation.
- **REQ-FID-029** The product, reports and docs shall be localisable, and shall ship in the major metrology markets' languages with reviewed terminology.
- **REQ-FID-030** Agent assistance shall be backed by curated skills and a published, reproducible evaluation of agents on metrology tasks, including adversarial safety tasks.
- **REQ-FID-031** All apps, reports, docs and the website shall share one design system, and every primary screen shall be designed and usability-tested with practising metrologists.

## Interfaces

Users reach the product through the desktop apps, the `fid` CLI, MCP servers (`fid mcp`), Python and the loopback control channel. Everything else is internal to the domains below.

## Split rationale

The split is by **capability domain**, not by app or phase. Shared foundations (CORE, GEO, KRN, IO, VIZ, AUTO, HAL) are separate from the evaluation domains (ALN, GDT, UNC, SCAN), the workflow domains (CMM, LVM, RPT, IND) and the trust and community domains (TRUST, ECO).

This lets one capability serve every focused app. For example, alignment is written once and used by the scan-inspection, CMM and large-volume apps. The apps themselves are thin compositions, defined in ECO (decision D-004).

Alternatives considered:
- **Split by app** (Inspect / Probe / Network). Rejected: it duplicates the kernel, alignment and GD&T across apps.
- **Split by phase.** Rejected: phases change and capabilities don't.

## Open questions

- None (`D-001` through `D-052` are accepted; architectural and specification decisions proceed autonomously under `D-052`).

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE | Core platform: registry, journal, project model | 004 005 017 021 | GOM.PRJ, PW.PRJ |
| GEO | Geometry and spatial data | 010 016 020 | GOM.CAD, SA.CAD.03 |
| IO | Import, export and interoperability | 001 010 | GOM.IMP/EXP, PW.IMP/EXP, PCD.CAD.01/EXP, SA.CAD.01 |
| KRN | Metrology kernel: numerics, fitting, construction | 006 007 | GOM.CON, PW.CON, PCD.CON, SA.REL |
| ALN | Alignment and registration | 001 019 | GOM.ALN, PW.ALN, PCD.ALN, SA.XFM |
| GDT | Dimensional and GD&T evaluation | 001 008 009 | GOM.DIM/GDT, PW.GDT, PCD.DIM/CAD.02, SA.GDT |
| UNC | Uncertainty, decision rules, verification data | 003 007 009 018 | SA.UNC |
| SCAN | Scan processing and surface inspection | 001 | GOM.MSH/SRF, PW.PCL/CMP, PCD.LAS.02, SA.SCN/CAD.02 |
| CMM | CMM programming and execution | 002 011 021 | PCD.PRB/FEA/SCN/PTH/CAD.03, PW.CMM |
| LVM | Large-volume and multi-instrument networks | 003 | SA.NET, SA.BLD |
| HAL | Instrument integration (hardware abstraction) | 002 003 011 014 024 | SA.INS, PW.PRB/SCN, PCD.MCH/LAS.01/LAS.03, DEV |
| VIZ | GPU viewer and UX | 005 006 016 | GOM.VIZ, PW.VIZ, SA.VIZ |
| AUTO | Automation surfaces: CLI, MCP, Python, control channel | 004 005 014 021 | GOM.AUT, PW.AUT, PCD.PRG, SA.MP |
| RPT | Reporting, statistics and results data | 001 012 | GOM.RPT/TRD, PW.MSP/RPT, PCD.RPT, SA.RPT |
| IND | Industry analysis packs | 013 | GOM.SPE, PW.IND, PCD.IND |
| TRUST | Signing and independent verification | 018 | none |
| ECO | Ecosystem, apps, SDK and community | 013 014 015 017 | none |
| OUTR | Outreach: website, docs portal, launch and adoption | 022 | none |
| PLAT | Platform support: OS, hardware, GPU, deployment | 017 023 | none |
| FLOW | End-to-end workflows, personas, acceptance suite | 025 | JOB |
| MIG | Migration from incumbents and result correlation | 026 | none |
| QUAL | Customer validation, compliance and security | 027 | none |
| ENT | Enterprise server, identity and integrations | 028 | none |
| I18N | Internationalisation and localisation | 029 | none |
| AGENT | Agent skills, recipes and evaluation | 030 | none |
| UX | Design system, interaction design, user research | 031 | none |
