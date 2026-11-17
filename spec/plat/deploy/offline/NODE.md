---
id: PLAT.DEPLOY.OFFLINE
title: Air-gapped install bundle and no-egress test
level: 3
parent: PLAT.DEPLOY
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PLAT.DEPLOY-001, REQ-PLAT.DEPLOY-002, REQ-PLAT.DEPLOY-003]
covers: []
depends_on: [PLAT.MATRIX, ECO.REL, ECO.CI]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

- **Offers:** offline bundle layout requirements for ECO.REL; install manifest format; no-egress CI job definitions (added to the platform job list); the network-feature list.
- **Consumes:** ECO.REL bundles; ECO.CI runners with network sandboxing; `PLAT.MATRIX` Tier 1 set; a reference non-device workflow (likely the P1a transcription-report example project).

## Decomposition hints

BUNDLE (bundle layout and install manifest format; leaf candidate), NOEGRESS (sandboxed CI test per OS: network namespace on Linux, firewall rules on Windows, a sandbox profile on macOS), NETLIST (generated list of network-using features from the registry, if commands declare network use).

## Open questions

- Whether commands should declare "uses network" in the registry manifest (so NETLIST is generated) is a CORE.REG question; raised here, not decided.
- Recording DNS attempts on macOS CI runners may need a privileged helper [unverified].

## Children

_None yet: status `open`._
