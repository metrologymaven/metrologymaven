---
id: ENT.OPS
title: Server deployment: containers, backup, restore and upgrades
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-ENT-006]
covers: []
depends_on: [ENT.SRV, ECO.REL.SIGN, ECO.REL.PROC, CORE.JRN.CHAIN]
cli: [fid server *-backup, fid server *-upgrade, fid server check-health]
mcp: [server_list_backups, server_check_health]
journal: [server.*_backup@*, server.*_upgrade@*]
updated: 2026-11-16
---

# ENT.OPS: Server deployment: containers, backup, restore and upgrades

## Purpose

Makes the server something a factory IT team can run on its own hardware: signed container images, reference Kubernetes and single-host manifests, backup and restore that preserve every journal, and upgrades that lose no data and can be rolled back. It is a separate unit because operations work is independent of server features and has its own acceptance tests (restore drills, upgrade drills).

## Scope

**In:**
- Container images for the server, signed with the release keys, with SBOM
- Reference deployments: single host (compose-style) and Kubernetes manifests
- Air-gapped installation of the server from an offline bundle
- Backup (online, consistent) and restore, with verification of journal chains after restore
- Upgrades with data migration, pre-upgrade backup and rollback
- Health and readiness checks
- Operations documentation

**Out** (and which node owns it instead):
- Server features and stored data (`ENT.SRV`)
- Release signing, SBOM generation and release channels (`ECO.REL`)
- Desktop installers and offline desktop bundles (`ECO.REL.PKG`, `PLAT.DEPLOY`)

## Requirements

- **REQ-ENT.OPS-001** Each server release shall ship as container images for linux/amd64 and linux/arm64 [unverified that arm64 is required], signed and with an SBOM produced by `ECO.REL.SIGN`.
- **REQ-ENT.OPS-002** The project shall publish a single-host and a Kubernetes reference deployment, each tested in CI by starting the server and passing the health check.
- **REQ-ENT.OPS-003** When a backup is restored onto a new server, every archived journal shall pass chain verification and the catalog content digest shall equal the one recorded at backup time.
- **REQ-ENT.OPS-004** A backup taken while the server is accepting writes shall be consistent: it shall contain no partial journal entry.
- **REQ-ENT.OPS-005** When the server is upgraded, it shall take a verified backup first, migrate data, and lose no committed entry; an upgrade drill over the fixture archive shall show identical journal hashes before and after.
- **REQ-ENT.OPS-006** If an upgrade fails, then the operator shall be able to roll back to the previous release and the pre-upgrade backup with no data loss.
- **REQ-ENT.OPS-007** The server shall install and run with no outbound internet access.
- **REQ-ENT.OPS-008** Starting a restore or an upgrade shall be A0 and never projected to MCP; listing backups and the health check shall be R0.

## Interfaces

- **Offers:** container images, deployment manifests, `server.*_backup`, `server.*_upgrade`, `server.check_health@1`; operations guide.
- **Consumes:** server storage layout (`ENT.SRV`), release signing and SBOM (`ECO.REL.SIGN`), release process and versioning (`ECO.REL.PROC`), chain verification (`CORE.JRN.CHAIN`).

## Decomposition hints

IMAGE (container images), DEPLOY (reference manifests and CI), BACKUP (backup and restore drills), UPGRADE (migrations and rollback), HEALTH.

## Open questions

- CONSTITUTION §2 lists the shipped binaries and does not include a server; the images are assumed to run `fid server ...` (see `ENT.SRV`).
- Supported server OS and architecture belong in the support matrix (`PLAT.MATRIX`), which has no server row yet.

## Children

_None yet: status `open`._
