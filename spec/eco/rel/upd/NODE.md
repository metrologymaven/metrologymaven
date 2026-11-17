---
id: ECO.REL.UPD
title: Opt-in update check
level: 3
parent: ECO.REL
kind: module
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REL-007]
covers: []
depends_on: [ECO.REL.PROC, CORE.STO]
cli: [fid update *]
mcp: [update_*]
journal: [update.*]
updated: 2026-11-16
---

# ECO.REL.UPD: Opt-in update check

## Purpose

The in-product way to learn that a newer release exists on the user's channel: off by default (C7), enabled only by an explicit user setting, verifying the channel feed's signature before showing anything, and never downloading or installing by itself. It is the only part of ECO.REL that runs inside the product.

## Scope

**In:**
- Update-check setting (default off) and channel selection
- `update.check@1`: fetch and verify the channel feed, compare versions, report
- Showing the result; linking to the download page
- Offline behaviour: no error noise when there is no network

**Out** (and which node owns it instead):
- Channel feeds and their signing (owned by `ECO.REL.PROC` and `ECO.REL.SIGN`)
- Installing updates (people do that, or enterprise tooling under `PLAT.DEPLOY`)
- Settings storage (owned by `CORE.STO`)

## Requirements

- **REQ-ECO.REL.UPD-001** A fresh installation shall have the update check disabled and shall make no network request for updates until the user enables it.
- **REQ-ECO.REL.UPD-002** The update check shall be a registry command of side-effect class X1 (data leaves the machine), so agents can only request it, never run it directly (CONSTITUTION §3).
- **REQ-ECO.REL.UPD-003** If the channel feed's signature does not verify against the bundled release public key, then the check shall report "feed not trusted" and show no version information from it.
- **REQ-ECO.REL.UPD-004** The update request shall send no identifier other than what the transport itself needs (no user, machine, install or usage identifiers).
- **REQ-ECO.REL.UPD-005** The product shall never download or install an update without a separate explicit user action.
- **REQ-ECO.REL.UPD-006** Enabling or disabling the update check shall be a journaled settings change.

## Interfaces

- **Offers:** `update.check@1` (X1; CLI `fid update check`; MCP request-only `update_check`), `update.set_policy@1` (settings change; class to be fixed at leaf level).
- **Consumes:** channel feeds (ECO.REL.PROC); release public keys (ECO.REL.SIGN); CORE.STO settings.

## Decomposition hints

POLICY (setting and channel), CHECK (fetch, verify, compare), UI (notification).

## Open questions

- Whether changing the update policy is W2 (journaled document state) or A0 (administrator configuration) is not in CONSTITUTION §3.1; enterprise deployments (PLAT.DEPLOY) may want to lock it. Settle at leaf level, or by decision if it needs a constitution change.

## Children

_None yet: status `open`._
