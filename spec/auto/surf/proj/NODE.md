---
id: AUTO.SURF.PROJ
title: Surface projection model and name derivation
level: 3
parent: AUTO.SURF
kind: task
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-surface]
built_pr: 249
satisfies: [REQ-AUTO.SURF-001]
covers: []
depends_on: [CORE.REG]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF.PROJ: Surface projection model and name derivation

## Purpose

A pure function from the registry manifest to one `SurfaceProjection` per registered command major: its CLI path, MCP tool name, Python name, MCP projection, parameters with quantity kinds and units, result type, side-effect class and determinism class. Every surface emitter and the completeness check consume this one model, so no surface derives a name or a projection on its own.

## Scope

**In:**
- `SurfaceProjection` data type and its canonical serialisation
- Name derivation for CLI, MCP and Python from the command ID (CONSTITUTION §3)
- MCP projection derived from the side-effect class table, cross-checked against the manifest's declared projection
- Name-collision detection across all commands

**Out:**
- Emitting files from the projection (`AUTO.SURF.GEN`)
- Completeness and staleness checks (`AUTO.SURF.CHECK`)
- Manifest export and declaration validation (`CORE.REG`)
- Runtime enforcement of classes (`CORE.BUS`)

## Requirements

- **REQ-AUTO.SURF.PROJ-001** The projection shall take only the manifest bytes as input and shall record the manifest's SHA-256 digest in its output.
- **REQ-AUTO.SURF.PROJ-002** The projection shall derive the CLI path of `domain.verb_object@major` as `fid domain verb-object`, replacing underscores with hyphens in the verb-object part.
- **REQ-AUTO.SURF.PROJ-003** The projection shall derive the MCP tool name by dropping `@major` and replacing dots with underscores, appending `_vN` when the major N is 2 or more, and shall reject any name outside `[a-z0-9_]` or longer than 64 characters.
- **REQ-AUTO.SURF.PROJ-004** The projection shall derive the Python name of a command as module `fiducial.<domain>` and function `<verb_object>`, appending `_vN` when the major N is 2 or more.
- **REQ-AUTO.SURF.PROJ-005** The projection shall map side-effect classes to MCP projections as R0, R1 and I1 to `exposed`; W1 and W2 to `named`; W3, X1 and D1 to `request_only`; M1, S1 and A0 to `none`.
- **REQ-AUTO.SURF.PROJ-006** If the manifest's declared MCP projection for a command differs from the class mapping, then projection shall fail with error `projection_mismatch` naming the command.
- **REQ-AUTO.SURF.PROJ-007** If two commands derive the same CLI path, MCP tool name or Python name, then projection shall fail with error `name_collision` naming both commands.
- **REQ-AUTO.SURF.PROJ-008** The projection list shall be ordered by command ID and major in byte order, and serialised as RFC 8785 canonical JSON.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json` (one command per side-effect class, one command at major 2), when `project(manifest)` runs, then the output equals the golden file `fixtures/projection/min-classes.golden.json` byte for byte, including `projection: "none"` for the M1, S1 and A0 commands and the name `align_run_bestfit_v2` for the major-2 command.
- **AC-2** Given `align.run_bestfit@1`, then the CLI path is `fid align run-bestfit`, the MCP name `align_run_bestfit` and the Python name `fiducial.align.run_bestfit`.
- **AC-3** (error path) Given fixture `fixtures/manifest/bad-projection.json` where an M1 command declares `exposed`, when projection runs, then it fails with `projection_mismatch` naming that command and returns no projection.
- **AC-4** (error path) Given fixture `fixtures/manifest/collision.json` where `demo.run_fit@2` and `demo.run_fit_v2@1` both derive the MCP name `demo_run_fit_v2`, then projection fails with `name_collision` naming both IDs.
- **AC-5** (error path) Given a command whose derived MCP name has 65 characters, then projection fails with `name_too_long`.
- **AC-6** (determinism) Given the same manifest bytes, two projections on Windows and Linux produce byte-identical output.

## Design

- **Registry command(s):** none; library function `project(manifest: &[u8]) -> Result<Projection, ProjError>`.
- **Data structures:** `Projection { manifest_digest: Sha256, commands: Vec<SurfaceProjection> }`; `SurfaceProjection { command_id, major, cli_path: Vec<String>, mcp_name, py_module, py_name, mcp_projection: Exposed|Named|RequestOnly|None, side_effect, determinism, params: Vec<ParamSpec>, result: TypeRef, long_running: bool, dry_run: bool, help: DocText }`. `ParamSpec` carries name, type, quantity kind, unit, optional default and a `path_like` flag (consumed by `AUTO.SURF.ROOTS`).
- **Algorithm:** parse manifest with a bounded JSON reader (max 64 MiB, max depth 64); validate ID grammar; derive names; map classes; check collisions with ordered maps (`BTreeMap`); serialise with an RFC 8785 serializer.
- **Errors:** `manifest_parse`, `bad_command_id`, `projection_mismatch`, `name_collision`, `name_too_long`, `bad_name_chars`.
- **Determinism:** D0. Pure function of the input bytes; no hashing with random state, no locale, no clock.
- **Performance budget:** 5,000 commands projected in < 200 ms on reference hardware [unverified until measured].
- **Crate:** `fid-surface`

## Test fixtures

Synthetic manifests written by hand under `fixtures/manifest/` (CC-BY-4.0, project-authored): `min-classes.json`, `bad-projection.json`, `collision.json`, `long-name.json`; golden output under `fixtures/projection/`.

## Build prompt

> In crate `fid-surface` (Rust, edition 2021), implement `pub fn project(manifest: &[u8]) -> Result<Projection, ProjError>`. Input is a registry manifest in RFC 8785 canonical JSON. Output: one `SurfaceProjection` per command major, ordered by command ID then major, plus the manifest's SHA-256 digest. Derivations: CLI `domain.verb_object@major` → `["fid", domain, verb-object]` (underscores to hyphens in the verb-object); MCP name = ID without `@major`, dots to underscores, plus `_vN` for major N ≥ 2, must match `^[a-z0-9_]{1,64}$`; Python = module `fiducial.<domain>`, function `<verb_object>` plus `_vN` for N ≥ 2. Class to MCP projection: R0/R1/I1 exposed, W1/W2 named, W3/X1/D1 request_only, M1/S1/A0 none; fail with `projection_mismatch` if the manifest declares otherwise. Fail with `name_collision` naming both commands if any derived name repeats. Serialise the projection as RFC 8785 canonical JSON. Acceptance: golden-file equality for `fixtures/manifest/min-classes.json`; error cases `projection_mismatch`, `name_collision`, `name_too_long`; byte-identical output across OSes. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; bounded parsing (64 MiB, depth 64) that fails closed; use `BTreeMap`, never `HashMap`, so output order is deterministic. Files: `crates/fid-surface/src/projection.rs`, `src/names.rs`, tests under `crates/fid-surface/tests/`.

## Open questions

- None.

## Children

_Leaf: none._
