---
id: CORE.REG.MAN
title: Canonical manifest export and digests
level: 3
parent: CORE.REG
kind: feature
status: leaf
phase: P0
size: M
owners: []
claimed_by:
built_in: [fid-registry, fid-registry-build]
built_pr: 253
satisfies: [REQ-CORE.REG-005, REQ-CORE.REG-006, REQ-CORE.REG-007]
covers: []
depends_on: [CORE.REG.DEF, CORE.REG.VAL, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: [fid registry show-manifest]
mcp: [registry_show_manifest]
journal: [registry.show_manifest@1]
updated: 2026-11-19
---

# CORE.REG.MAN: Canonical manifest export and digests

## Purpose

Turns the validated declarations into the one canonical Registry Manifest: RFC 8785 JSON with a SHA-256 digest, plus a digest per command definition. Every generated surface, the bus and every journal session header refer to this manifest by digest. The leaf also provides the R0 command that shows the manifest in force.

## Scope

**In:** manifest document structure, canonical serialisation, manifest digest, per-definition digests, the manifest JSON schema, the `registry.show_manifest@1` command.
**Out:** declaration validation (`CORE.REG.VAL`); compatibility gating (`CORE.REG.VER`); list, describe and diff commands (`CORE.REG.QRY`); recording the digest in session headers (`CORE.JRN.ENTRY`); per-MCP-tool definition manifests (`AUTO`).

## Requirements

- **REQ-CORE.REG.MAN-001** When the manifest is exported, the registry shall serialise one object holding the manifest format version, the quantity-catalogue digest and the command definitions sorted by ID then major, as RFC 8785 canonical JSON.
- **REQ-CORE.REG.MAN-002** The manifest digest shall be the SHA-256 of the exact exported bytes, written as `sha256:` followed by 64 lowercase hex digits.
- **REQ-CORE.REG.MAN-003** The exported manifest shall contain no timestamp, file path, host name, user name or build-environment value, so two exports from the same declarations are byte-identical.
- **REQ-CORE.REG.MAN-004** Each command definition in the manifest shall carry a `definition_digest` equal to the SHA-256 of the RFC 8785 form of that definition with the `definition_digest` field omitted.
- **REQ-CORE.REG.MAN-005** The exported manifest shall validate against the published manifest JSON schema.
- **REQ-CORE.REG.MAN-006** When `fid registry show-manifest` runs, the system shall return the manifest digest and, with `--full`, the manifest bytes, as side-effect class R0.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/core/reg/decls/valid.json`, when the manifest is exported twice in separate processes, then both outputs are byte-identical and their digest equals the value in `fixtures/core/reg/manifest/valid.digest`.
- **AC-2** Given the same declarations supplied in reversed order, when exported, then the bytes are identical to AC-1.
- **AC-3** Given the exported manifest, when each `definition_digest` is recomputed independently with a second JCS implementation (the TypeScript conformance runner), then every digest matches.
- **AC-4** Given the exported manifest, when validated against `manifest.schema.json`, then it passes; given `fixtures/core/reg/manifest/tampered.json`, then `fid registry show-manifest --check <file> --json` reports `manifest_invalid`.
- **AC-5** Given a running core, when `fid registry show-manifest --json` runs, then it prints `{ "digest": "sha256:..." }`, writes one Activity Log entry `registry.show_manifest@1` and no Record Journal entry.

## Design

- **Registry command(s):** `registry.show_manifest@1`, R0, MCP `exposed`; params `{ full: bool = false }`; result `{ digest: String, bytes?: String }`.
- **Data structures:** `Manifest { format: u32, quantity_catalogue_digest, commands: Vec<CommandDefJson> }`; `Digest` newtype for `sha256:<hex>`.
- **Algorithm:** convert declarations to JSON values; compute each definition digest via JCS; sort; serialise with `fid-canon`; hash with SHA-256.
- **Errors:** `manifest_invalid` (schema or digest mismatch on a supplied manifest).
- **Determinism:** D0; canonical JSON, sorted inputs, no environment data.
- **Performance budget:** export of 5,000 definitions in < 500 ms [unverified until measured].
- **Crate:** `fid-registry` (module `manifest`).
- **Pinned choices (build):**
  - **Entry points:** `export(&[CommandDecl]) -> Result<(Vec<u8>, Digest), ManifestError>` as the build prompt asks, over `Catalogue::geo_frame()`; `export_with(&[CommandDecl], &Catalogue) -> Result<Export, ManifestError>` with `Export { bytes, digest, definitions: BTreeMap<(id, major), Digest> }`; `check_manifest(&[u8], &Catalogue) -> Result<Digest, ManifestError>`; `manifest_schema()`; `ManifestInForce` (an export plus its catalogue); `command_def()`, `show_manifest`, `invoke_cli`, `invoke_mcp` for `registry.show_manifest@1`. `Digest` is `fid_types::Digest` (AGENTS.md rule 14), not a new newtype. The module's error is `fid_registry::manifest::ManifestError`; the crate-root name `ManifestError` stays `compat`'s reader error until `CORE.REG.VER.COMPAT` moves to this module.
  - **Validation, no bypass:** `export_with` first refuses more than 50,000 declarations (`too_many_commands`), then runs `CORE.REG.VAL` (`validate_with`, default `Limits`) against the catalogue passed in. Any VAL error fails the export as `declarations_invalid` carrying VAL's sorted error list, so a duplicate (ID, major) (`duplicate_command`), two IDs with one tool name (`tool_name_collision`), an unknown quantity kind (`unknown_quantity`), a `number` without `x-quantity` (`missing_quantity`) and every other VAL rule stop the export. There is no flag, feature or entry point that exports without it.
  - **Quantity catalogue:** an input, never defined here (REQ-CORE.REG.DEF-003, REQ-CORE.REG.DEF.SCHEMA-004). `quantity_catalogue_digest` is `Catalogue::digest()` of the catalogue the declarations were validated against. `export` uses `Catalogue::geo_frame()`; a catalogue document that extends it is read with `Catalogue::from_json`, which keeps GEO.FRAME's kinds unchanged. This leaf publishes no kind: `GEO.FRAME.UNITS` says kinds beyond length and angle are owned by the domains that need them.
  - **Domain quantity catalogues (resolved, maintainer decision; was an open question):** a crate whose schemas name kinds beyond `length` and `angle` publishes a fragment `{"format": "fid-quantity-kinds/1", "kinds": [...]}` (kinds in the `CORE.REG.DEF.SCHEMA` catalogue form, strictly sorted by name; at most 1 MiB, 5 levels, no duplicate key) as a static string returned by one function next to its declarations export. `fid-registry-build` lists them in `QUANTITY_SOURCES` (sorted by crate name, the fixed merge order; today `fid-jobs` with `fid_jobs::quantity_catalogue`) and `merge_catalogues` adds them to `Catalogue::geo_frame()` add-only: a fragment kind named like a GEO kind is `geo_kind_redefined` (even when identical: GEO kinds and Π are locked); a kind an earlier crate defined with another value is `quantity_kind_conflict` naming both crates (an identical definition is accepted once); after each crate the whole catalogue is re-read through `Catalogue::from_json`, so every shape rule and symbol uniqueness across kinds apply, and a refusal is `catalogue_invalid` naming that crate with the catalogue code. The merged catalogue is sorted by kind, so non-conflicting fragments give one digest in any order. `workspace_catalogue()` is that merge over `QUANTITY_SOURCES`; its digest is the workspace manifest's `quantity_catalogue_digest`. `fid-jobs` publishes `time` (canonical `s`; `h` = 3600, `min` = 60, `ms` = 1/1000) for `job.wait_job@1` `timeout`, and `dimensionless` (unit `1` only) for the `progress` field of the four job commands, which is a fraction in 0..1, not a step count, so no `count` kind is published; `progress` carries `x-quantity: "dimensionless"` (also in `schema/job-status.schema.json`). The workspace now exports; a test pins it, and that `Catalogue::geo_frame()` alone still refuses the five job fields as `unknown_quantity`.
  - **Normal form and digests:** each declaration is parsed as a `CommandDef` (VAL guarantees it parses) and written with `CommandDef::to_value`: `human_only` false, `network` `none` and `cli_bare` false written out when absent, `exposure` in surface order `cli, mcp, python, gui, control`, every other field as declared. `definition_digest` = `sha256:` + SHA-256 of the RFC 8785 bytes of that normal form (REQ-004). Commands sorted by `id` (byte order), then `major` (REQ-001). The document is `{"commands", "format": 1, "quantity_catalogue_digest"}` serialised by `fid-canon`; the manifest digest is SHA-256 of exactly those bytes (REQ-002). No clock, path, host, user or environment value is read (REQ-003). An export above 64 MiB (`fid-canon`'s parse limit) is `manifest_too_large`.
  - **Schema:** `schema/manifest.schema.json` (next to `command-def.schema.json`) is the RFC 8785 bytes of `manifest_schema()` plus a newline (a test compares them): top level exactly `commands`, `format` (`const 1`), `quantity_catalogue_digest` (digest pattern); each command is `command-def.schema.json`'s record under `$defs/command`, with `definition_digest` added and `human_only`, `network`, `cli_bare`, `definition_digest` required (normal form). Sorting, uniqueness, digests and VAL's rules cannot be said in a schema and are checked by `check_manifest`.
  - **Check (`manifest_invalid`):** larger than 64 MiB; not JSON or a duplicate key; not byte-identical to its RFC 8785 form; top-level fields other than the three; `format` not 1; `quantity_catalogue_digest` malformed or not the digest of the catalogue in force; `commands` not an array of objects; a `definition_digest` missing, malformed or not matching; `id`/`major` malformed; commands not strictly ascending by (`id`, `major`); any VAL error (VAL runs once: the backstop re-export reuses that result through the private `export_validated` and does not validate again); and, as a backstop, any byte difference from re-exporting the same definitions (so a definition not in normal form is refused). Messages name indices and rule codes and never echo input.
  - **Command:** `registry.show_manifest@1`, R0, `exposed`, D0, idempotency `natural`, fs_effect `none`, undo `not_undoable`, exposure `cli`, `mcp`, web `available`. Params `{ full: bool = false, check?: string }` (`additionalProperties: false`; `check` at most 67,108,864 characters, counted as characters because JSON Schema `maxLength` counts characters, and the schema description says so; its UTF-8 bytes are bounded separately by `check_manifest`, above 64 MiB `manifest_invalid`); result `{ digest, bytes? }`. Without `check` the result is the digest of the manifest in force and, with `full`, its bytes as a string; with `check` the supplied manifest text is verified by `check_manifest` against the catalogue in force and its digest (and with `full` its text) is returned, or `manifest_invalid`. CLI `fid registry show-manifest [--full] [--check <file>] [--json]`: `--check` reads the file through a reader the caller supplies (fid-registry does no I/O) and passes its text as `check`; output is canonical JSON either way; unknown or repeated options and more than 8 arguments are `bad_params`. MCP `registry_show_manifest` takes at most 64 MiB of parameters, depth 4, duplicate keys refused. Both surfaces push one `ActivityEntry { command, surface }` per call into the caller's Activity Log and write nothing else (R0: no Record Journal entry). Error codes: `declarations_invalid`, `too_many_commands`, `manifest_too_large`, `catalogue_invalid`, `canonical_json`, `manifest_invalid`, `bad_params`, `unknown_command`.
  - **Build step (`fid-registry-build`, layer services):** `fid-registry` cannot depend on the crates that declare commands (they depend on it), so a new crate collects them: `SOURCES`, sorted by crate name, lists every crate on main with command definitions (`fid-align`, `fid-dep`, `fid-jobs`, `fid-registry`, `fid-report`) through their public definition functions; `build(sources, &Catalogue)` turns them into `CommandDecl`s and calls `export_with`. A failing definition function is `declaration_build_failed` naming the crate. The binary `fid-registry-build [--catalogue <file>]` writes the manifest bytes to stdout and the digest to stderr (exit 0), or the RFC 8785 error report to stderr (exit 1; usage or catalogue file 2; the file is read with a 1 MiB bound). Without `--catalogue` it uses `workspace_catalogue()` (a merge refusal is exit 1). A crate that adds commands adds one `SOURCES` entry; the `declarations()` export of `CORE.REG.DEF.MACRO` can replace the function pointers when it lands. `fid-fit`'s command data rows (`fit.command_row@1`) are not registry declarations and are not collected (see Open questions).
  - **Fixtures:** `fixtures/core/reg/manifest/make_fixtures.py` (stdlib only, `--check` in CI) computes `valid.manifest.json` and `valid.digest` from `decls/valid.json`, `order.json` / `order.manifest.json` / `order.digest` (normal form: exposure out of order, optional fields omitted, a precondition and an algorithm, two majors, shuffled) and `tampered.json` (one title changed, digest kept), with its own RFC 8785 writer and `hashlib`; the catalogue digest is read from `def/schema/catalogue/geo_frame_units.sha256`. AC-3's second implementation is `check_digests.mjs` (node, the canonicaliser of `fixtures/core/jrn/canon/runner.mjs`); there is no TypeScript runner on main. `tools/check_manifest_schema.py` validates the golden manifests against the schema with Python `jsonschema`. AC-1's "separate processes" runs the test binary twice as child processes.
  - **Performance:** 5,000 definitions (copies of the largest `valid.json` declaration, 6.9 MB of manifest) exported in 0.77 to 1.36 s (release build, one shared desktop x86_64 machine under load, 2026-11-19), of which VAL alone took 0.33 to 0.50 s in the same runs; the 500 ms budget is not met on that machine and stays [unverified].

## Test fixtures

`fixtures/core/reg/decls/valid.json` (shared with CORE.REG.VAL), `fixtures/core/reg/manifest/valid.digest` (golden digest, regenerated only by an explicit fixture update), `fixtures/core/reg/manifest/tampered.json`. All CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `manifest` with `pub fn export(decls: &[CommandDecl]) -> Result<(Vec<u8>, Digest), ManifestError>` and register the command `registry.show_manifest@1` (side-effect R0, MCP projection exposed, determinism D0, params `{ full: bool = false }`, result `{ digest, bytes? }`). The manifest is one object `{ format, quantity_catalogue_digest, commands }`, commands sorted by (id, major), each with `definition_digest` = SHA-256 of the RFC 8785 form of the definition without that field. Serialise with the `fid-canon` crate (RFC 8785; 64-bit integers as decimal strings). Manifest digest = SHA-256 of the exact bytes, formatted `sha256:<64 lowercase hex>`. No timestamps, paths or environment data. Publish `manifest.schema.json`. Tests: AC-1..AC-5 (byte-identical exports, order independence, cross-implementation digest check, schema validation, R0 Activity-Log-only behaviour). Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), deterministic collections only, allowlisted dependencies.

## Open questions

- Whether the manifest should also embed the per-MCP-tool definition digests that AUTO generates, or AUTO publishes them separately; this leaf leaves them to AUTO.
- `fid-fit` declares fit commands as data rows (`fit.command_row@1`, KRN.LSQ.TRAIT) "that CORE.REG reads", but no leaf turns a row into a `CORE.REG.DEF.FIELD` record (title, description, idempotency, exposure, lifecycle, web are missing from a row). Until one does, fit commands are not in the manifest.
- `CORE.REG.VER.COMPAT`'s stopgap `Manifest::from_json` reader and `CORE.REG.VER.GATE`'s `--current` file can now move to this module (`check_manifest` and `fid-registry-build`); not done here, to keep this PR to one leaf.
- `--check` is carried as an additive `check` parameter (manifest text) so the CLI and MCP share one handler and the command stays fs_effect `none`; Design listed only `full`.

## Children

_Leaf: none._
