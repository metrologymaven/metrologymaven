---
id: IND.CONTRACT
title: Pack contract and lifecycle
level: 2
parent: IND
kind: subsystem
status: open
phase: P3
size: L
owners: []
claimed_by:
satisfies: [REQ-IND-001, REQ-IND-002, REQ-IND-004, REQ-IND-005]
covers: []
depends_on: [ECO, CORE]
cli: [fid pack list-packs, fid pack show-pack, fid pack install-pack, fid pack enable-pack, fid pack disable-pack, fid pack remove-pack, fid pack verify-pack]
mcp: [pack_list_packs, pack_show_pack, pack_verify_pack]
journal: [pack.*_pack]
updated: 2026-11-16
---

# IND.CONTRACT: Pack contract and lifecycle

## Purpose

Defines what an industry pack is and how the platform hosts it: the pack manifest, the subset of public extension points a pack may use, the standards-and-methods declaration, the reference-dataset conformance harness, the stamping of pack identity onto results, and the behaviour of projects when a pack is missing or disabled. Every reference pack (the siblings of this node) is built against this contract and nothing else, which is what lets community domain experts own packs without engine access.

## Scope

**In:**
- Pack manifest schema: pack ID, version, maintainer(s), declared standards and methods, registered commands, element types, checks, report templates, reference datasets
- The allowed extension-point subset for packs (a profile over the ECO SDK)
- Pack management commands: list, show, install, enable, disable, remove, verify
- Reference-dataset conformance harness (`fid pack verify-pack`)
- Stamping pack identity and version onto every ResultRecord a pack produces
- Replay of pack results, including version-mismatch outcomes
- Read-only display with provenance of results whose pack is missing or disabled

**Out** (and which node owns it instead):
- The generic plugin mechanism: packaging, signing, sandboxing, compatibility policy (owned by `ECO`)
- Registry, journal and project container (owned by `CORE`)
- Any industry-specific geometry or evaluation (owned by the sibling pack nodes `IND.AIRFOIL`, `IND.GEAR`, `IND.SHEET`, `IND.GAPFL`, `IND.TUBE`, `IND.PATTERN`, `IND.PORT`)
- Report layout engine (owned by `RPT`; packs only register templates)

## Requirements

- **REQ-IND.CONTRACT-001** The pack contract shall define a machine-readable manifest that lists every command, element type, check and report template a pack registers, and the host shall reject at load time any registration not listed in the manifest.
- **REQ-IND.CONTRACT-002** If a pack attempts to reach an engine interface outside the published pack extension-point profile, then the host shall refuse to load the pack and report the offending interface by name.
- **REQ-IND.CONTRACT-003** The manifest shall declare each standard or method the pack implements as a designator plus edition (or a method ID with version for non-standard methods), and the host shall refuse to load a manifest whose declared standard lacks an edition.
- **REQ-IND.CONTRACT-004** The manifest shall name at least one maintainer, and `fid pack show-pack` shall display the maintainer(s) and the declared standards.
- **REQ-IND.CONTRACT-005** The manifest shall reference at least one reference dataset with expected results and per-value acceptance tolerances, and the host shall refuse to load a pack that declares none.
- **REQ-IND.CONTRACT-006** When `fid pack verify-pack` runs, the system shall evaluate every reference dataset of the pack and report each expected value as pass or fail against its declared tolerance, exiting non-zero if any value fails.
- **REQ-IND.CONTRACT-007** Every ResultRecord produced by a pack command shall carry the pack ID and pack version alongside the algorithm ID and version required by CONSTITUTION §5.
- **REQ-IND.CONTRACT-008** When a journal containing pack commands is replayed with the same pack version installed, the replay outcome shall follow CONSTITUTION §4 exactly as for core commands.
- **REQ-IND.CONTRACT-009** If a journal is replayed with a different version of a pack than the one recorded, then the system shall flag every affected result and shall classify any changed conformance decision as `DIVERGENT`.
- **REQ-IND.CONTRACT-010** If a project references a pack that is not installed or is disabled, then the system shall open the project without error and show that pack's stored results read-only with their provenance (pack ID, version, algorithm ID, journal entry hash).
- **REQ-IND.CONTRACT-011** While a pack is missing or disabled, the system shall refuse to re-evaluate or edit that pack's characteristics and shall name the missing pack and version in the refusal.
- **REQ-IND.CONTRACT-012** Pack install, enable, disable and remove shall be registry commands that are journaled, and none of them shall be callable by an agent through MCP.

## Interfaces

- **Offers:** the pack manifest schema; the pack extension-point profile; `pack.list_packs@1`, `pack.show_pack@1`, `pack.verify_pack@1` (R0/R1, exposed on MCP); `pack.install_pack@1`, `pack.enable_pack@1`, `pack.disable_pack@1`, `pack.remove_pack@1` (human only); journal events `pack.<verb>_pack`.
- **Consumes:** ECO plugin packaging, signing and loader; CORE registry, journal and ResultRecord; RPT template registration point.

## Decomposition hints

MANIFEST (schema and load-time validation), PROFILE (extension-point allowlist for packs), LIFE (install/enable/disable/remove), VERIFY (reference-dataset harness), PROV (pack identity stamping and replay), ORPHAN (missing-pack read-only behaviour).

## Open questions

- Side-effect class for pack install/enable/disable: A0 (admin configuration) seems right because packs add executable code, but ECO has not yet fixed the class for generic plugins. REQ-012 only states the agent restriction; align the class once ECO's SDK node is expanded.
- Whether third-party (non-first-party) packs may register new element types or only checks on existing ones. Assumed allowed, subject to ECO sandboxing.

## Children

_None yet: status `open`._
