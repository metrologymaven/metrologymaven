---
id: ECO
title: Ecosystem: app family, SDK, plugins and community machinery
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-013, REQ-FID-014, REQ-FID-015, REQ-FID-017]
covers: []
depends_on: [CORE, AUTO]
cli: [fid plugin *]
mcp: [plugin_*]
journal: [plugin.*]
updated: 2026-11-16
---

# ECO: Ecosystem and community machinery

## Purpose

Everything that makes the project grow by itself, following the patterns proven by the Craft app family (see `research/artcraft-design-lessons.md`):
- the **family of focused apps** on shared libraries;
- the plugin and SDK system;
- repository layout and layering enforcement;
- shared house rules, synced into every repo;
- shared CI and bot review;
- generated parity reports;
- release engineering;
- docs;
- contributor onboarding and governance tooling;
- this spec tree's own tooling.

## Scope

**In:**
- App family definition: which focused apps exist and how they compose shared crates (D-004)
- Plugin and SDK system: extension points, packaging, signing, sandboxing, compatibility
- Repository topology: core, libs, apps, contrib, common-actions, spec
- Build-time layering enforcement
- House rules and their sync with digest checks
- Shared CI and bots (lint, test, licence allowlist, CodeRabbit-style review)
- Generated parity report from the registry against seeds
- Packaging and installers for Windows, Linux and macOS, plus self-host web
- Release channels and signed releases
- Docs site and book
- Example corpora and benchmark datasets
- Contributor ladder and RFC tooling
- Spec-tree tooling (scope.py, grow.ps1)

**Out:**
- Individual industry packs (IND)
- Automation surfaces (AUTO)
- Governance policy text (GOVERNANCE.md, decided by D-003)

## Requirements

- **REQ-ECO-001** The project shall define a family of focused apps (at minimum: scan inspection, CMM programming, large-volume networks, reporting) that are thin compositions of shared crates and contain no duplicated domain logic.
- **REQ-ECO-002** Third parties shall extend the system through a versioned SDK and plugin interface, with a documented compatibility policy and without forking.
- **REQ-ECO-003** Crate layering shall be enforced at build time, so a lower layer (e.g. KRN) cannot depend on a higher one (e.g. VIZ).
- **REQ-ECO-004** Contributor and agent rules shall live in one canonical source and be synced into every repository as a generated block with a digest. CI shall fail on edited blocks and warn on stale ones.
- **REQ-ECO-005** Every PR shall pass shared CI: format, lint (including the never-crash lints), tests, licence allowlist, spec-tree check, plus automated review.
- **REQ-ECO-006** A parity report shall be generated from the live registry against the seed inventories and published with each release. It shall never be hand-edited.
- **REQ-ECO-007** Releases shall ship signed installers for Windows, Linux and macOS plus a self-hostable web build, with checksums and a documented release process.
- **REQ-ECO-008** The project shall publish public benchmark datasets and example projects that contributors can use to reproduce results and performance numbers.
- **REQ-ECO-009** Every unit of work shall be traceable to a spec node, and the spec-tree tooling shall let any contributor or agent find, claim, expand and validate nodes without maintainer help.
- **REQ-ECO-010** The documentation shall state each app's status and known gaps plainly, generated from the spec tree and parity report where possible.

## Interfaces

- **Offers:** the SDK; plugin packaging; repo templates; house-rules blocks; shared CI actions; parity and coverage reports; installers.
- **Consumes:** CORE registry manifest, AUTO generators, the spec tree.

## Decomposition hints

APPS (app family), SDK (plugins and extension points), REPO (topology and layering), RULES (house rules and sync), CI (shared CI and bots), PARITY (generated parity), REL (packaging and releases), DOCS, DATA (benchmarks and examples), COMM (contributor ladder, RFC tooling, spec-tree tooling).

## Split rationale

The split follows the **project's delivery pipeline**, one subsystem per artefact or gate a contribution passes through: what is shipped (APPS, SDK), how code is laid out and constrained (REPO, RULES), how a change is checked (CI), how progress is reported (PARITY), how it is released (REL), what it is measured against (DATA), and how work is found, claimed and governed (COMM). Each child owns the logic of its own rule; ECO.CI only runs the checks that REPO, RULES and COMM define, so no check has two owners.

Boundaries with siblings: the docs portal, website and publication of benchmarks belong to OUTR, so the "DOCS" hint was not made a child; ECO keeps only the generated status and gaps data behind REQ-ECO-010 (ECO.APPS declares it, ECO.PARITY generates it). The support matrix, tiers and field deployment belong to PLAT; ECO.REL only builds and signs artefacts for the targets PLAT names.

Alternatives considered:
- **Split by repository** (core, libs, contrib, common-actions, spec). Rejected: the repository set is itself a decision inside REPO and will change; responsibilities would move when repos split.
- **Separate DOCS child.** Rejected: it would duplicate OUTR's docs portal.
- **Separate spec-tree tooling child from contributor and RFC tooling.** Rejected for now to stay within 9 children; both serve REQ-ECO-009 and share inputs. ECO.COMM may split them at level 3.
- **Fold RULES into CI.** Rejected: the rules sync is a write-side tool used outside CI and has its own size and digest contract.

## Open questions

- D-004 (app family) blocks ECO.APPS, which is set to `status: decision`. D-005 (UI toolkit and the TypeScript boundary) affects ECO.APPS and ECO.REPO. D-003 (governance) affects ECO.COMM thresholds and who holds release signing keys in ECO.REL. D-001 (licence) sets the SPDX allowlist used by ECO.CI and plugin licence rules in ECO.SDK. D-002 (name) must settle before ECO.REL ships a public installer.
- REQ-ECO-007 lists Windows, Linux and macOS; ECO.REL ties installers to PLAT Tier 1 targets instead, so the set follows the support matrix.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.APPS | App family: focused apps composed from shared crates | 001 010 | none |
| ECO.SDK | Plugin system and SDK | 002 | none |
| ECO.REPO | Repository topology and build-time layering | 003 | none |
| ECO.RULES | House rules and their sync into every repository | 004 | none |
| ECO.CI | Shared CI actions and automated review | 005 | none |
| ECO.PARITY | Generated parity, coverage and status reports | 006 010 | none |
| ECO.REL | Packaging, signed installers and release channels | 007 | none |
| ECO.DATA | Benchmark datasets and example projects | 008 | none |
| ECO.COMM | Contributor workflow, RFC tooling and spec-tree tooling | 009 | none |
