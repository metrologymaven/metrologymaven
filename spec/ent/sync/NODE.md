---
id: ENT.SYNC
title: Desktop-server synchronisation, offline operation and conflicts
level: 2
parent: ENT
kind: subsystem
status: open
phase: P1b
size: XL
owners: []
claimed_by:
satisfies: [REQ-ENT-001, REQ-ENT-003]
covers: []
depends_on: [CORE.JRN.CHAIN, CORE.JRN.BR, CORE.STO.CAS, CORE.STO.PRIV, CORE.BUS.POL, ENT.SRV, ENT.IAM]
cli: [fid server sync-*, fid server *-remote, fid server *-conflict]
mcp: [server_sync_*, server_*_remote, server_*_conflict]
journal: [server.sync_*@*, server.*_remote@*, server.*_conflict@*]
updated: 2026-11-16
---

# ENT.SYNC: Desktop-server synchronisation, offline operation and conflicts

## Purpose

Keeps desktops and the server in step without making the desktop depend on the server. The desktop remains the place where work happens; sync pushes committed journal entries and blobs up, pulls released library entries and other users' project branches down, and turns divergent histories into branches for a person to resolve. It is a separate unit because its correctness rule (nothing lost, nothing duplicated, conflicts shown, never merged silently) is distinct from storage and identity.

## Scope

**In:**
- Configuring a remote server for a desktop project (remote set, show, remove)
- Push of committed journal entries and their blobs; pull of server-held branches and library entries
- Offline queue: commits made while unreachable are pushed later in order
- Idempotent transfer keyed by entry hash and blob digest; resumable after interruption
- Divergence detection and representation as branches on the desktop journal; conflict listing and resolution through branch promotion
- Side-effect classes: push is X1 (data leaves the machine), pull into a branch is W1
- Sync status for the GUI and `fid doctor`

**Out** (and which node owns it instead):
- Storing archived journals and server catalog indexing (`ENT.SRV`)
- Sign-in and desktop credentials (`ENT.IAM`)
- Branch diff and promotion mechanics (`CORE.JRN.BR`)
- Re-executing branch commands onto a moved head (`CORE.RPL.RETGT`)
- Shared-folder catalogs without a server (`RPT.CATALOG.SHARE`)

## Requirements

- **REQ-ENT.SYNC-001** While the server is unreachable, every desktop command shall behave as it does with no server configured, and sync shall queue committed entries without blocking any command.
- **REQ-ENT.SYNC-002** When a desktop reconnects, sync shall push every queued committed entry in journal order so that the server holds each entry exactly once, identified by its entry hash.
- **REQ-ENT.SYNC-003** If a transfer is interrupted at any point, then the next sync shall resume and finish with no lost and no duplicated entry or blob; a fixture of 1,000 random interruption points shall show zero losses and zero duplicates.
- **REQ-ENT.SYNC-004** When the server and desktop hold different successors of the same journal entry, sync shall keep both histories as named branches and shall not merge, reorder or drop any entry.
- **REQ-ENT.SYNC-005** When a conflict exists, the system shall list it with both branch heads and their entity diff, and shall resolve it only through a branch promotion by a principal allowed to promote.
- **REQ-ENT.SYNC-006** Push shall be side-effect class X1 and request-only on MCP, and pull into a branch shall be W1.
- **REQ-ENT.SYNC-007** Sync shall verify the hash chain of every received entry before storing it locally, and shall refuse and report any entry that fails.
- **REQ-ENT.SYNC-008** No data shall leave the desktop through sync unless the user has configured a remote for that project; a project with no remote shall make no network connection.

## Interfaces

- **Offers:** `server.sync_push@1`, `server.sync_pull@1`, `server.sync_status@1` (names refined at leaf level), `server.*_remote` and `server.*_conflict` commands; sync status events for the GUI.
- **Consumes:** chain verification (`CORE.JRN.CHAIN`), branches and promotion (`CORE.JRN.BR`), blobs (`CORE.STO.CAS`), offline and consent rules (`CORE.STO.PRIV`), egress policy (`CORE.BUS.POL`), archive write API (`ENT.SRV`), authenticated session (`ENT.IAM`).

## Decomposition hints

REMOTE (remote configuration and credentials handle), PUSH (queue and push), PULL (pull into branches, library fetch), RESUME (idempotent transfer and resumption), CONFLICT (divergence detection, listing, resolution), STATUS.

## Open questions

- Whether a human-started push from the CLI counts as a human act depends on D-012; push is X1 either way.
- Large scan blobs may make first push slow; chunk size and bandwidth limits are left to the leaf level [unverified].

## Children

_None yet: status `open`._
