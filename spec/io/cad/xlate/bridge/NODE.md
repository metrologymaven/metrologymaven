---
id: IO.CAD.XLATE.BRIDGE
title: Native CAD import command with STEP hand-off
level: 4
parent: IO.CAD.XLATE
kind: feature
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-IO.CAD.XLATE-002, REQ-IO.CAD.XLATE-003]
covers: [GOM.IMP.06, PCD.CAD.01, PW.IMP.03, SA.CAD.01]
depends_on: [IO.CAD.XLATE.DESC, IO.REG.PLUGIN, IO.CAD.STEP.CMD, IO.CAD.PMI.CMD, CORE.STO.CAS]
side_effect: W2
determinism: D1
cli: [fid import cad-native]
mcp: [import_cad_native]
journal: [import.cad_native@1]
updated: 2026-11-17
---

# IO.CAD.XLATE.BRIDGE: Native CAD import command with STEP hand-off

## Purpose

`import.cad_native@1`: resolves the native format of the source, picks an installed translator plugin, runs it in the plugin host, and, for translators that output STEP AP242, imports the intermediate STEP through exactly the same path and checks as a first-party STEP import (geometry, and PMI when requested). Provenance records the native file digest, the intermediate STEP digest and the translator's identity.

## Scope

**In:**
- `import.cad_native@1` (W2): `source`, `translator?` (plugin ID; required when several match), `pmi?`, `stage?`, `dry_run?`, `limits?`
- Running the translator through `IO.REG.PLUGIN` (out-of-process native host) with source bytes only
- Storing native bytes and intermediate STEP bytes as blobs; handing the STEP blob to the `IO.CAD.STEP.CMD` library entry, or to `IO.CAD.PMI.CMD` when `pmi` is set
- Healer plugins (D-024 option C): a translator whose format is `step` re-writes a STEP file; invoked the same way on a STEP source
- Dispatch to `IO.CAD.XLATE.DIRECT` when the translator's output kind is `direct`
- Provenance: translator plugin ID, version, package digest, native digest, intermediate STEP digest

**Out:**
- STEP and PMI reading (`IO.CAD.STEP.*`, `IO.CAD.PMI.*`)
- Direct-entity validation (`IO.CAD.XLATE.DIRECT`)

## Requirements

- **REQ-IO.CAD.XLATE.BRIDGE-001** When a translator outputs STEP AP242, the command shall import the output through the same code path and checks as `import.cad_step@1` (or `import.cad_pmi@1` with `pmi`), and record both the native file digest and the intermediate STEP digest in the provenance.
- **REQ-IO.CAD.XLATE.BRIDGE-002** If no installed translator covers the source's native format, then the command shall fail with `translator_missing` naming the format and journal nothing.
- **REQ-IO.CAD.XLATE.BRIDGE-003** If more than one installed translator covers the format and `translator` is not given, then the command shall fail with `translator_ambiguous` listing the plugin IDs.
- **REQ-IO.CAD.XLATE.BRIDGE-004** If the translator crashes, hangs or exceeds its caps, then the command shall fail with `reader_crashed` or `limit_exceeded` and leave the document unchanged.
- **REQ-IO.CAD.XLATE.BRIDGE-005** The import provenance shall record the translator plugin ID, version and package digest.

## Acceptance criteria

- **AC-1** Given the sample translator plugin `fixtures/io/cad/xlate/bridge/fake_jt` (returns the bytes of `fixtures/io/cad/step/brep/cube_ap242.stp` for any input) and `fixtures/io/cad/xlate/bridge/part.jt`, when `fid import cad-native --source <handle> --json` runs, then one body with 6 faces is created and the provenance holds the `.jt` digest, the STEP digest and the plugin ID, version and package digest.
- **AC-2** Given the AC-1 STEP bytes imported directly with `fid import cad-step`, then the bodies are identical except for provenance.
- **AC-3** (error path) Given `part.CATPart` and no CATIA translator, then `translator_missing{format: catia_v5}` and nothing is journaled.
- **AC-4** (error path) Given two installed `jt` translators and no `--translator`, then `translator_ambiguous` listing both IDs.
- **AC-5** (error path) Given the sample plugin in crash mode, then `reader_crashed` and the document is unchanged.
- **AC-6** (replay) Given the journal of AC-1 and the same plugin installed, when replayed exactly, then `IDENTICAL`; with the plugin uninstalled, the replay outcome is `NOT_REPLAYABLE` naming the plugin.

## Design

- **Registry commands:** `import.cad_native@1` (W2; params above; result `StepImportReport` or `PmiImportReport` plus `TranslatorInfo`). CLI `fid import cad-native`, MCP `import_cad_native`.
- **Algorithm:** resolve format (DESC) → choose translator → bytes to blob → plugin host run (bytes in, STEP bytes or direct records out) → STEP blob → STEP/PMI library entry → one transaction.
- **Errors:** `translator_missing`, `translator_ambiguous`, `reader_crashed`, `limit_exceeded`, `plugin_output_invalid`, plus STEP and envelope errors.
- **Determinism:** D1 for the first-party part; the translator's own determinism is whatever the plugin declares, so replay uses the stored intermediate STEP blob when present [unverified: replay policy agreed with CORE.RPL].
- **Performance budget:** overhead beyond the translator and STEP import < 1 s [unverified until measured].
- **Crate:** `fid-io-cad-xlate`.

## Test fixtures

A fake translator plugin built from project code under `fixtures/io/cad/xlate/bridge/` (MIT OR Apache-2.0); dummy native files (arbitrary bytes with native extensions, CC-BY-4.0). No real native files are needed.

## Build prompt

> In crate `fid-io-cad-xlate`, implement `import.cad_native@1` (W2; CLI `fid import cad-native`; MCP `import_cad_native`). Resolve the native format by extension (`translator_missing{format}`), pick the translator (explicit `translator` or the single match; else `translator_ambiguous{ids}`), store native bytes as a blob, run the plugin through the `IO.REG.PLUGIN` out-of-process host with bytes only, and on `step_ap242` output store the STEP blob and import it through the `fid-io-cad` STEP (or PMI when `pmi`) library entry with identical checks; on `direct` output hand over to the direct-output module. Record translator ID, version, package digest, native digest and STEP digest in provenance. Crashes and caps → `reader_crashed`/`limit_exceeded`, document unchanged. No native-format parsing in this crate (D-019). CONSTITUTION §8: no panic, no unwrap. Tests AC-1..AC-6 with a fake translator plugin.

## Open questions

- Replay of a native import when the translator is no longer installed: proposed `NOT_REPLAYABLE` for exact replay, with the stored intermediate STEP usable for a re-targeted replay; to confirm with CORE.RPL.

## Children

_Leaf: none._
