---
id: IND.CONTRACT.LIFE
title: Pack lifecycle commands
level: 3
parent: IND.CONTRACT
kind: module
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND.CONTRACT-004, REQ-IND.CONTRACT-012]
covers: []
depends_on: [IND.CONTRACT.MANIFEST, IND.CONTRACT.PROFILE, ECO.SDK]
cli: [fid pack list-packs, fid pack show-pack, fid pack install-pack, fid pack enable-pack, fid pack disable-pack, fid pack remove-pack]
mcp: [pack_list_packs, pack_show_pack]
journal: [pack.list_packs@1, pack.show_pack@1, pack.install_pack@1, pack.enable_pack@1, pack.disable_pack@1, pack.remove_pack@1]
updated: 2026-11-16
---

# IND.CONTRACT.LIFE: Pack lifecycle commands

## Purpose

The registry commands that list, show, install, enable, disable and remove industry packs. They wrap the ECO.SDK plugin lifecycle and add the pack-specific checks (manifest and profile validation) and displays (maintainers, standards, datasets).

## Scope

**In:**
- `pack.list_packs@1` and `pack.show_pack@1` (R0, exposed on MCP)
- `pack.install_pack@1`, `pack.enable_pack@1`, `pack.disable_pack@1`, `pack.remove_pack@1` (A0, never on MCP)
- Running manifest and profile validation on install and enable
- Offline install from a local package file

**Out** (and which node owns it instead):
- Package format, signature check and code loading (owned by `ECO.SDK`)
- Running reference datasets (owned by `IND.CONTRACT.VERIFY`)
- What happens to projects after disable or remove (owned by `IND.CONTRACT.ORPHAN`)

## Requirements

- **REQ-IND.CONTRACT.LIFE-001** `pack.install_pack@1`, `pack.enable_pack@1`, `pack.disable_pack@1` and `pack.remove_pack@1` shall be A0 registry commands that are journaled and never projected to MCP (CONSTITUTION §3.1).
- **REQ-IND.CONTRACT.LIFE-002** `pack.list_packs@1` and `pack.show_pack@1` shall be R0 commands exposed on MCP.
- **REQ-IND.CONTRACT.LIFE-003** When `fid pack show-pack <pack-id>` runs, the system shall display the pack version, maintainers, declared standards with editions, declared method IDs, registered items and reference datasets.
- **REQ-IND.CONTRACT.LIFE-004** When a pack is installed or enabled, the system shall run manifest validation and profile checks first and shall leave the pack disabled if either fails.
- **REQ-IND.CONTRACT.LIFE-005** When a pack is removed or disabled, the system shall not delete or alter any stored result produced by that pack.
- **REQ-IND.CONTRACT.LIFE-006** The system shall install a pack from a local package file with no network access and no account.

## Interfaces

- **Offers:** the six commands above; journal events `pack.<verb>_pack@1`.
- **Consumes:** ECO.SDK plugin install, enable and uninstall; `IND.CONTRACT.MANIFEST` validation; `IND.CONTRACT.PROFILE` checks.

## Decomposition hints

READ (list and show, R0) and ADMIN (install, enable, disable, remove, A0). Both could be leaves once ECO.SDK's lifecycle child is specified.

## Open questions

- Whether `pack.*` admin commands should be thin aliases of `plugin.*` commands or separate registry commands; aliases are not allowed (CONSTITUTION §3), so they are specified here as separate commands that call the plugin lifecycle.

## Children

_None yet: status `open`._
