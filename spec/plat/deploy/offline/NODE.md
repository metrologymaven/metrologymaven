---
id: PLAT.DEPLOY.OFFLINE
title: Air-gapped install bundle and no-egress test
level: 3
parent: PLAT.DEPLOY
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-001, REQ-PLAT.DEPLOY-002, REQ-PLAT.DEPLOY-003]
covers: []
depends_on: [PLAT.MATRIX]
uses_artifacts: [ECO.REL.PROC.CHANNEL, ECO.REL.PKG.PLAN, CORE.REG.MAN, ECO.DATA.EX]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# PLAT.DEPLOY.OFFLINE: Air-gapped install and operation

## Purpose

Guarantees that the platform installs and runs on a machine with no network at all: an offline bundle per Tier 1 platform holds the apps, the documentation and every runtime dependency, and a CI test proves that neither install nor normal operation tries to reach the network without opt-in. Shop floors and defence suppliers often run air-gapped; this node makes that a tested property, not a hope.

## Scope

**In:**
- Offline bundle contents per Tier 1 platform: installers, offline docs, runtime dependencies (e.g. platform runtimes the installer would otherwise download), an install manifest with SHA-256 per file
- The install manifest format (also read by `fid doctor --check-install`)
- The no-egress CI test: install, start each app, run a fixed non-device workflow, all inside a network sandbox that records any connection attempt
- The list of features that need network and are opt-in (update check, telemetry, dataset fetch)

**Out** (and which node owns it instead):
- Building and signing installers and bundles (`ECO.REL`)
- Offline update bundles (`PLAT.DEPLOY.UPDATE`)
- Per-user install rights (`PLAT.DEPLOY.NOADMIN`)
- Network policy of individual features (each feature's owner; this node only tests the default)

## Requirements

- **REQ-PLAT.DEPLOY.OFFLINE-001** The project shall publish, per Tier 1 platform and release, one offline bundle containing the installers for every app, the `fid` CLI, the offline documentation and every runtime dependency the installers need.
- **REQ-PLAT.DEPLOY.OFFLINE-002** Each offline bundle shall contain an install manifest listing every installed file with its SHA-256, in a versioned machine-readable format.
- **REQ-PLAT.DEPLOY.OFFLINE-003** When the offline bundle is installed on a Tier 1 machine with all network interfaces disabled, the install shall complete and every app shall start and complete the reference non-device workflow.
- **REQ-PLAT.DEPLOY.OFFLINE-004** When a release is built, CI shall run install, app start and the reference non-device workflow in a network sandbox on each Tier 1 platform and fail if any outbound connection attempt (DNS lookup included) is recorded.
- **REQ-PLAT.DEPLOY.OFFLINE-005** The project shall publish the list of features that use the network, each of which shall be off by default and enabled only by explicit user opt-in.

## Interfaces

- **Offers:** `offline-bundle.json` (bundle layout, schema 1) and `install-manifest.json` (schema 1; read by `PLAT.DOCTOR.HOSTCHK.INSTALL` and `PLAT.DEPLOY.UPDATE`); `platform/ref-workflow.toml` (the reference non-device workflow, read by `PLAT.DEPLOY.NOADMIN.AUDIT`); the no-egress job entries; `docs/deploy/network-features.md` generated from the registry manifest.
- **Consumes (artifacts):** the release manifest (`ECO.REL.PROC.CHANNEL`) and package plan (`ECO.REL.PKG.PLAN`); the registry manifest (`CORE.REG.MAN`); the P1a example project (`ECO.DATA.EX`). Code: Tier 1 set from `PLAT.MATRIX`.

## Decomposition hints

See Children.

## Open questions

- Resolved: "uses network" needs no new registry field. Since D-031 and CORE.BUS.POL.EGRESS, only X1, I1 and D1 commands may open sockets, so `PLAT.DEPLOY.OFFLINE.NETLIST` generates the list from command classes in the registry manifest.
- Recording DNS attempts on macOS CI runners may need a privileged helper [unverified]; carried by `PLAT.DEPLOY.OFFLINE.NOEGRESS`.

## Split rationale

Split by **artefact**: a bundle layout and install manifest format (consumed by ECO.REL, the doctor and the updater), a CI job that proves no egress, and a generated network-feature list. They change for different reasons (packaging, sandboxing tooling, registry contents) and are each a small leaf.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| PLAT.DEPLOY.OFFLINE.BUNDLE | Offline bundle layout, install manifest format and bundle check | 001 002 | none |
| PLAT.DEPLOY.OFFLINE.NOEGRESS | Reference workflow and sandboxed no-egress CI job | 003 004 | none |
| PLAT.DEPLOY.OFFLINE.NETLIST | Network-feature list generated from the registry manifest | 005 | none |
