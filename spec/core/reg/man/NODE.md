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
satisfies: [REQ-CORE.REG-005, REQ-CORE.REG-006, REQ-CORE.REG-007]
covers: []
depends_on: [CORE.REG.DEF, CORE.REG.VAL, CORE.JRN.CANON]
side_effect: R0
determinism: D0
cli: [fid registry show-manifest]
mcp: [registry_show_manifest]
journal: [registry.show_manifest@1]
updated: 2026-11-16
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

## Test fixtures

`fixtures/core/reg/decls/valid.json` (shared with CORE.REG.VAL), `fixtures/core/reg/manifest/valid.digest` (golden digest, regenerated only by an explicit fixture update), `fixtures/core/reg/manifest/tampered.json`. All CC-BY-4.0.

## Build prompt

> In crate `fid-registry`, add module `manifest` with `pub fn export(decls: &[CommandDecl]) -> Result<(Vec<u8>, Digest), ManifestError>` and register the command `registry.show_manifest@1` (side-effect R0, MCP projection exposed, determinism D0, params `{ full: bool = false }`, result `{ digest, bytes? }`). The manifest is one object `{ format, quantity_catalogue_digest, commands }`, commands sorted by (id, major), each with `definition_digest` = SHA-256 of the RFC 8785 form of the definition without that field. Serialise with the `fid-canon` crate (RFC 8785; 64-bit integers as decimal strings). Manifest digest = SHA-256 of the exact bytes, formatted `sha256:<64 lowercase hex>`. No timestamps, paths or environment data. Publish `manifest.schema.json`. Tests: AC-1..AC-5 (byte-identical exports, order independence, cross-implementation digest check, schema validation, R0 Activity-Log-only behaviour). Constraints: CONSTITUTION §8 (no panic, no unsafe, deny unwrap/expect), deterministic collections only, allowlisted dependencies.

## Open questions

- Whether the manifest should also embed the per-MCP-tool definition digests that AUTO generates, or AUTO publishes them separately; this leaf leaves them to AUTO.

## Children

_Leaf: none._
