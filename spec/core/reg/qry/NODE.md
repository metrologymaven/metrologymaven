---
id: CORE.REG.QRY
title: Registry query commands
level: 3
parent: CORE.REG
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry, fid-registry-build]
built_pr: 394
satisfies: [REQ-CORE.REG-011, REQ-CORE.REG-012]
covers: []
depends_on: [CORE.REG.MAN, CORE.REG.VER]
side_effect: R0
determinism: D0
cli: [fid registry list-commands, fid registry describe-command, fid registry diff-manifests]
mcp: [registry_list_commands, registry_describe_command, registry_diff_manifests]
journal: [registry.list_commands@1, registry.describe_command@1, registry.diff_manifests@1]
updated: 2026-11-16
---

# CORE.REG.QRY: Registry query commands

## Purpose

The read-only commands through which people, scripts and agents discover what the platform can do: list commands, describe one command, and diff two manifests to see what changed between releases. They are the progressive-disclosure entry point for agents and the release-notes source for maintainers.

## Scope

**In:** `registry.list_commands@1`, `registry.describe_command@1`, `registry.diff_manifests@1`, their filters, output shapes and errors.
**Out:** manifest export and `show_manifest` (`CORE.REG.MAN`); the compatibility classifier and diff engine (`CORE.REG.VER`); the generated "describe then invoke" MCP dispatcher (`AUTO`).

## Requirements

- **REQ-CORE.REG.QRY-001** When `registry.list_commands@1` runs, the system shall return every non-hidden command with ID, major, title, lifecycle state, side-effect class and MCP projection, sorted by ID then major.
- **REQ-CORE.REG.QRY-002** When `registry.list_commands@1` is given `domain`, `side_effect` or `lifecycle` filters, the system shall return only commands matching all given filters, and shall include hidden majors only when `include_hidden` is true.
- **REQ-CORE.REG.QRY-003** When `registry.describe_command@1` names an existing ID and major, the system shall return the full definition and its `definition_digest`.
- **REQ-CORE.REG.QRY-004** If `registry.describe_command@1` names an unknown ID or major, then the system shall return a typed `not_found` error listing up to five closest IDs by edit distance.
- **REQ-CORE.REG.QRY-005** When `registry.diff_manifests@1` is given two manifests, the system shall return added, removed, lifecycle-changed and changed commands, each change marked additive or breaking, sorted by ID then major.
- **REQ-CORE.REG.QRY-006** If a manifest given to `registry.diff_manifests@1` fails schema validation or its recomputed digest differs from its stated digest, then the system shall return a typed `manifest_invalid` error.
- **REQ-CORE.REG.QRY-007** All three commands shall be side-effect class R0 and shall write only to the Activity Log.

## Acceptance criteria

- **AC-1** Given the manifest from fixture `fixtures/core/reg/decls/valid.json`, when `fid registry list-commands --json` runs, then the output lists every non-hidden command in (ID, major) order and no hidden one.
- **AC-2** Given the same manifest, when `fid registry list-commands --side-effect W2 --json` runs, then only W2 commands are listed.
- **AC-3** Given `fixtures/core/reg/manifest/v1.json` and `v2.json` (one added command, one removed, one deprecated, one with a new optional parameter, one with a changed parameter type), when `fid registry diff-manifests v1.json v2.json --json` runs, then the report has exactly those five entries, the parameter-type change is marked breaking and the optional-parameter change additive.
- **AC-4** (error path) Given `fid registry describe-command align.run_bestfi@1`, then the command fails with `not_found` and suggests `align.run_bestfit`.
- **AC-5** (error path) Given `fixtures/core/reg/manifest/tampered.json`, when diffed, then the command fails with `manifest_invalid` and writes no Record Journal entry.
- **AC-6** Given an agent principal over MCP, when it calls `registry_diff_manifests` with a raw path instead of an import-root handle, then the bus rejects the call (inherited from `CORE.BUS`); with a handle, the call succeeds.

## Design

- **Registry command(s):**
  - `registry.list_commands@1` R0, params `{ domain?: String, side_effect?: Class, lifecycle?: Lifecycle, include_hidden: bool = false }`, result `{ commands: [CommandSummary] }`.
  - `registry.describe_command@1` R0, params `{ id: String, major?: u32 }` (latest active major when omitted), result `{ definition, definition_digest }`.
  - `registry.diff_manifests@1` R0, `fs_effect: read_root`, params `{ a: FileHandle, b: FileHandle }`, result `{ changes: [Change] }`.
- **Data structures:** `CommandSummary`, `Change { id, major, kind: added|removed|lifecycle|changed, compat: additive|breaking, fields: [String] }`.
- **Algorithm:** filters over the in-memory manifest; diff delegates to `CORE.REG.VER`'s `Compat::classify`; edit distance is Levenshtein over IDs with deterministic tie-break by ID.
- **Errors:** `not_found`, `manifest_invalid`, `input_too_large` (manifest > 64 MB).
- **Determinism:** D0.
- **Performance budget:** list over 5,000 commands in < 50 ms [unverified until measured].
- **Crate:** `fid-registry` (module `query`).

## Test fixtures

`fixtures/core/reg/manifest/v1.json`, `v2.json`, `tampered.json`; synthetic, CC-BY-4.0. The expected diff is stored as `v1_v2.diff.json`.

## Build prompt

> In crate `fid-registry`, add module `query` registering three R0 commands: `registry.list_commands@1` (filters domain, side_effect, lifecycle, include_hidden; sorted by id then major), `registry.describe_command@1` (full definition plus definition_digest; unknown ID gives `not_found` with up to five nearest IDs by Levenshtein distance, ties broken by ID), and `registry.diff_manifests@1` (fs_effect read_root; two file handles; validate each manifest against `manifest.schema.json` and recompute its digest, else `manifest_invalid`; changes from the `Compat::classify` API of the registry's versioning module, marked additive or breaking). CLI names `fid registry list-commands | describe-command | diff-manifests`, MCP names `registry_list_commands | registry_describe_command | registry_diff_manifests`, all with `--json`. These commands write only to the Activity Log. Tests: AC-1..AC-6 with fixtures under `fixtures/core/reg/manifest/`. Constraints: CONSTITUTION §8 (no panic, no unsafe, bounded parsing), deterministic ordering, allowlisted dependencies.

## Open questions

- **D-035 (accepted 2026-11-17), built.** `registry.list_commands@1` and `describe_command@1` filter by the caller's audience: an MCP channel or `agent` principal sees only commands whose MCP projection (the bus's `mcp_projection`: declared projection, `human_only` clamped to `request_only`, class ceiling, `never` without the `mcp` surface) is not `never`. To such a caller every other command does not exist: never listed (also with `include_hidden`), `not_found` when described, never suggested.
- **Built in `fid-registry` (module `query`), registered in `fid-registry-build`.** AC-1 to AC-5 are tested with `fixtures/core/reg/qry/` (independent oracle `make_cases.py`); AC-6 is tested against the bus's `check_raw_paths` and projection check; resolving a handle below an import root is `CORE.BUS.POL.FS`, and the `fid registry ...` binary is `AUTO.CLI.TREE`. Pinned choices: `domain` is the ID's text before the first dot; `describe_command` without `major` takes the highest `active` major (none is `not_found`); suggestions use Levenshtein over Unicode scalar values; a diff entry per (ID, major) is `added` (additive), `removed` (breaking), `lifecycle` (breaking only when the new state is `removed`) or `changed`; a supplied manifest's quantity-catalogue digest is not compared with the catalogue in force, since an older release may carry another one. The performance budget is not measured.
- **#394 review, built.** For an agent, `describe_command` drops `lifecycle.replaced_by` when its target is not visible to the agent (`definition_digest` stays the digest of the full definition), and `diff_manifests` lists a change only when the command is not `never` on MCP in every manifest that holds it. The diff result carries `a_manifest_digest` and `b_manifest_digest`. The MCP projection rule lives once, in `fid-registry` (`mcp_projection`), and `fid-bus` re-exports it.
- **D-035 gap in CORE.REG.MAN (not this leaf).** `registry.show_manifest@1` is MCP-exposed and with `full: true` returns the whole manifest, every `never`-projected command included, to an agent. Its filtering belongs to CORE.REG.MAN; a follow-up is to be opened there.

## Children

_Leaf: none._
