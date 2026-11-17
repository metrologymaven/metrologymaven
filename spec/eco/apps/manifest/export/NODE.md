---
id: ECO.APPS.MANIFEST.EXPORT
title: Status, gaps and effective-command export
level: 4
parent: ECO.APPS.MANIFEST
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS.MANIFEST-005, REQ-ECO.APPS.MANIFEST-007]
covers: []
depends_on: [ECO.APPS.MANIFEST.SCHEMA, CORE.JRN.CANON]
uses_artifacts: [ECO.APPS.LIST, CORE.REG.MAN]
side_effect: W1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.MANIFEST.EXPORT: Status, gaps and effective-command export

## Purpose

Produces `apps-status.json`, the one machine-readable document that tools outside the app layer read instead of parsing manifests themselves: each app's declared status and scope, its known gaps, its enabled namespaces, the commands it effectively enables under a given registry manifest (with their definition digests), and whether its branding is still a placeholder. ECO.PARITY.STATUS, ECO.APPS.POLICY, the round-trip suite and release packaging all read it.

## Scope

**In:**
- Binary `fid-app-manifest export-status`
- Schema `apps-status.json` v1, canonical JSON, with a generated header
- Apps in the app list with no manifest, exported as `manifest: false`

**Out:**
- Validation rules (owned by `ECO.APPS.MANIFEST.SCHEMA`; the export refuses invalid manifests)
- Rendering status pages and consistency findings (owned by `ECO.PARITY.STATUS`)
- Refusing placeholder branding at release time (owned by `ECO.REL`, which reads the flag)

## Requirements

- **REQ-ECO.APPS.MANIFEST.EXPORT-001** The export shall contain one entry per app in `apps.toml`, sorted by `id`, each with `manifest` (bool) and, when a manifest exists, `status`, `status_scope`, `known_gaps`, `namespaces`, `commands` (`[{id, definition_digest, side_effect}]` sorted by ID) and `branding_placeholder`.
- **REQ-ECO.APPS.MANIFEST.EXPORT-002** The export shall be RFC 8785 canonical JSON whose header holds the generator name and version, the SHA-256 of each input manifest, of `apps.toml` and of the registry manifest, and no timestamp.
- **REQ-ECO.APPS.MANIFEST.EXPORT-003** If any manifest fails validation, then the export shall exit 1, print the findings and write no file.
- **REQ-ECO.APPS.MANIFEST.EXPORT-004** The entry field `branding_placeholder` shall be true whenever the manifest's branding has `internal = true`.

## Acceptance criteria

- **AC-1** Given `fixtures/manifest/export/` (manifests for `report` and `inspect`, `apps.toml` listing those two plus `probe` and `network`, and a registry manifest), when `fid-app-manifest export-status --manifests fixtures/manifest/export/manifests --apps fixtures/manifest/export/apps.toml --registry-manifest fixtures/manifest/export/registry-manifest.json --out <tmp>` runs, then `apps-status.json` matches the golden file byte for byte, with `probe` and `network` as `manifest: false`.
- **AC-2** (error path) Given an invalid `inspect` manifest, then the exit code is 1 and no file exists at `<tmp>/apps-status.json`.
- **AC-3** The header digests in AC-1 equal independently computed SHA-256 values of the four inputs.
- **AC-4** (determinism) Running AC-1 with the manifest directory listed in reverse order gives identical bytes; outputs on Windows, Linux and macOS are identical.

## Design

- **Registry command(s):** none; repository and release tool `fid-app-manifest export-status --manifests <dir> --apps <apps.toml> --registry-manifest <json> --out <dir>`. `side_effect: W1`: writes one file into a local output root.
- **Data structures:** `{ header: {generator, version, inputs: {manifests: {<id>: sha256}, apps_toml, registry_manifest}}, apps: [...] }`.
- **Algorithm:** validate each manifest with the schema validator; compute effective commands from the registry manifest; build the document with BTreeMaps; serialise with the canonical JSON library; write atomically.
- **Errors:** `EXPORT-INVALID` (with validator findings), `EXPORT-IO`.
- **Determinism:** D0.
- **Performance budget:** under 1 s for 10 apps and a 5 000-command registry [unverified until measured].
- **Crate:** `fid-app-manifest` (binary target).

## Test fixtures

Fixture manifests, `apps.toml`, registry manifest and golden output under `fixtures/manifest/export/` (MIT OR Apache-2.0).

## Build prompt

> In crate `fid-app-manifest` (implementation repository), add binary subcommand `export-status --manifests <dir> --apps <apps.toml> --registry-manifest <json> --out <dir>`. Validate every manifest with the crate's validator (exit 1 with findings and write nothing if any fails). Write `apps-status.json` as RFC 8785 canonical JSON (use the project's canonical JSON crate): header `{generator: "fid-app-manifest export-status", version, inputs: {manifests: {id: sha256}, apps_toml: sha256, registry_manifest: sha256}}`, no timestamp; `apps` sorted by id, one per `apps.toml` entry, with `manifest: bool` and, if present, `status`, `status_scope`, `known_gaps`, `namespaces`, `commands` (registry commands in enabled namespaces: `{id, definition_digest, side_effect}`, sorted) and `branding_placeholder` (= branding.internal). Write atomically. Golden-file tests for AC-1..AC-4 of spec node ECO.APPS.MANIFEST.EXPORT on three OSes. No `unwrap`/`expect`/`panic`/`unsafe`.

## Open questions

- None.

## Children

_Leaf: none._
