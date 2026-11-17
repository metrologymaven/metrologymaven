---
id: RPT.CATALOG.SHARE
title: Shared-folder catalogs
level: 3
parent: RPT.CATALOG
kind: module
status: split
phase: P1b
size: L
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG-001, REQ-RPT.CATALOG-007]
covers: [PW.MSP.03]
depends_on: [RPT.CATALOG.STORE, RPT.CATALOG.CHECK, RPT.CATALOG.QUERY, CORE.STO.SET, CORE.STO.LAYOUT, IO.REG.ROOTS]
cli: [fid stats attach-catalog, fid stats detach-catalog, fid stats query-shared-catalog]
mcp: [stats_attach_catalog, stats_detach_catalog, stats_query_shared_catalog]
journal: [stats.attach_catalog@1, stats.detach_catalog@1, stats.query_shared_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.SHARE: Shared-folder catalogs

## Purpose

Lets a team use one catalog kept on a file share: one owner maintains it, everyone else attaches it read-only. It is the local-first answer to central results management: no server, no account, just files with safe locking and integrity checks. A hosted or server-based results platform is outside this node.

## Scope

**In:**
- Catalog on a shared folder: owner identity, read-only attachment for non-owners
- Safe concurrent access (single writer, many readers) and detection of an unsafe file system
- Attaching and detaching a shared catalog alongside the local one; queries across both
- Integrity check on attach

**Out** (and which node owns it instead):
- Catalog schema and indexing (`RPT.CATALOG.STORE`)
- Hosted or server-based results management (`ENT.SRV`)
- Network egress to remote services (X1; none here)

## Requirements

- **REQ-RPT.CATALOG.SHARE-001** A shared catalog shall record its owner, and only the owner shall be able to register projects or re-index it.
- **REQ-RPT.CATALOG.SHARE-002** When a non-owner attaches a shared catalog, the system shall open it read-only and run the integrity check before using it.
- **REQ-RPT.CATALOG.SHARE-003** The shared catalog shall allow one writer and many readers at a time, and if the share does not support the required locking, then attach shall fail with error `stats.share_unsafe`.
- **REQ-RPT.CATALOG.SHARE-004** A shared catalog shall work with no network service other than the file share and no account (CONSTITUTION C7).
- **REQ-RPT.CATALOG.SHARE-005** Queries shall be able to span the local and attached catalogs and return a single deterministic order.

## Interfaces

- **Offers:** `stats.attach_catalog@1`, `stats.detach_catalog@1` (W1).
- **Consumes:** catalog store and check (siblings), storage primitives and settings (`CORE.STO`).

## Decomposition hints

LOCK (single-writer locking and file-system checks), ATTACH (attach, detach and read-only mode), FED (queries across catalogs).

## Open questions

- Resolved (agreed at review): central, server-based results management is owned by `ENT.SRV`; this node covers only the shared-folder part of PW.MSP.03.
- SQLite over network file systems has known locking hazards [unverified for SMB and NFS specifics]; `RPT.CATALOG.SHARE.LOCK` adds an explicit lock, read snapshots and a share probe, and its AC-5 must run on real shares.
- The cross-catalog query is a separate R0 command, `stats.query_shared_catalog@1` (within the parent's `stats.query_*` pattern), so `RPT.CATALOG.QUERY` stays single-catalog and no dependency cycle arises.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| RPT.CATALOG.SHARE.LOCK | Single-writer locking and file-share safety check (leaf) | 003 004 | none |
| RPT.CATALOG.SHARE.ATTACH | Attach and detach shared catalogs with owner rules (leaf) | 001 002 003 | PW.MSP.03 |
| RPT.CATALOG.SHARE.FED | Queries across local and attached catalogs (leaf) | 005 | none |
