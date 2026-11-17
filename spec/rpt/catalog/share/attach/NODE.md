---
id: RPT.CATALOG.SHARE.ATTACH
title: Attach and detach shared catalogs with owner rules
level: 4
parent: RPT.CATALOG.SHARE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-RPT.CATALOG.SHARE-001, REQ-RPT.CATALOG.SHARE-002, REQ-RPT.CATALOG.SHARE-003]
covers: [PW.MSP.03]
depends_on: [RPT.CATALOG.SHARE.LOCK, RPT.CATALOG.CHECK, RPT.CATALOG.STORE.REG, CORE.STO.SET, IO.REG.ROOTS]
side_effect: [W1, W1]
determinism: D0
cli: [fid stats attach-catalog, fid stats detach-catalog]
mcp: [stats_attach_catalog, stats_detach_catalog]
journal: [stats.attach_catalog@1, stats.detach_catalog@1]
updated: 2026-11-17
---

# RPT.CATALOG.SHARE.ATTACH: Attach and detach shared catalogs with owner rules

## Purpose

Lets a team share one catalog through a file share: the owner (the person who created it) maintains it; everyone else attaches it read-only next to their local catalog. Attaching probes the share, checks the catalog's integrity and records the attachment in the user's settings. This is the shared-folder part of central results management (seed PW.MSP.03); a hosted multi-user results server is `ENT.SRV`'s.

## Scope

**In:** `stats.attach_catalog@1`, `stats.detach_catalog@1`; owner identity stored in `catalog_meta`; read-only mode for non-owners (register, unregister and reindex refused); integrity check on attach; the list of attached catalogs in user settings.

**Out:** locking and probe (`RPT.CATALOG.SHARE.LOCK`); cross-catalog queries (`RPT.CATALOG.SHARE.FED`); server-held catalogs (`ENT.SRV`).

## Requirements

- **REQ-RPT.CATALOG.SHARE.ATTACH-001** A shared catalog shall record its owner identity at creation, and if a non-owner runs a register, unregister, reindex or rebuild command against it, then the command shall fail with error `stats.catalog_read_only`.
- **REQ-RPT.CATALOG.SHARE.ATTACH-002** When a non-owner attaches a shared catalog, the system shall probe the share, open the catalog (or its snapshot) read-only, run the integrity check, and attach it only if the probe is not `unsafe`.
- **REQ-RPT.CATALOG.SHARE.ATTACH-003** If the probe returns `unsafe`, then attach shall fail with error `stats.share_unsafe` and change nothing.
- **REQ-RPT.CATALOG.SHARE.ATTACH-004** Attach and detach shall be W1 commands journaled to the user settings journal, and agents shall name the share only by an approved root handle.
- **REQ-RPT.CATALOG.SHARE.ATTACH-005** The attach result shall report the integrity summary (OK, STALE and UNAVAILABLE counts) from the check.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/rpt/catalog/shared_folder` (a catalog owned by user A in a temporary folder), when user B runs `fid stats attach-catalog --share root://team/catalog --json`, then it is attached read-only and the integrity summary is returned.
- **AC-2** (error path) Given AC-1, when user B runs `register-catalog` against the shared catalog, then it fails with `stats.catalog_read_only`.
- **AC-3** (error path) Given the fault-injecting shim that makes the probe return `unsafe`, then attach fails with `stats.share_unsafe` and the settings are unchanged.
- **AC-4** When B runs `detach-catalog`, then the shared catalog is no longer listed and its files are unchanged.

## Design

- **Registry commands:** `stats.attach_catalog@1(share: FolderHandle, alias: Untrusted<Text>) -> { catalog_id, mode: ReadOnly | Owner, safety, integrity }` (W1); `stats.detach_catalog@1(catalog_id) -> ()` (W1). Both `--json`.
- **Data structures:** user setting `stats.attached_catalogs: Vec<{ catalog_id, alias, share }>`; `catalog_meta.owner`.
- **Errors:** `stats.share_unsafe`, `stats.catalog_read_only`, `stats.catalog_unavailable`, `stats.not_attached`.
- **Determinism:** D0 for the recorded state; the probe outcome is journaled as observed.
- **Performance budget:** attach of a 1 000 000-entry catalog including check under 90 s [unverified until measured].
- **Crate:** `fid-catalog` (module `share::attach`).

## Test fixtures

- `fixtures/rpt/catalog/shared_folder`: built by a test builder in a temporary folder from `three_projects`; CC-BY-4.0.

## Build prompt

> In crate `fid-catalog`, module `share::attach`, implement `stats.attach_catalog@1` and `stats.detach_catalog@1` (W1, journaled to the user settings journal, `--json`; CLI `fid stats attach-catalog`, `detach-catalog`; MCP names derived). Shared catalogs record their owner; non-owners get read-only mode and every write command against the catalog fails with `stats.catalog_read_only`. Attach: resolve the share handle (agents: approved roots only), run the share probe (`unsafe` → `stats.share_unsafe`, nothing changed), open the catalog or its snapshot read-only, run the integrity check, store the attachment in the user setting `stats.attached_catalogs`, return the integrity summary. Detach removes the setting only. Tests AC-1 to AC-4. CONSTITUTION §8 applies. Spec: `spec/rpt/catalog/share/attach/NODE.md`.

## Open questions

- None yet.

## Children

_Leaf: none._
