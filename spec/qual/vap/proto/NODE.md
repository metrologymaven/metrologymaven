---
id: QUAL.VAP.PROTO
title: IQ and OQ protocol templates and their command check
level: 3
parent: QUAL.VAP
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
built_in: [fid-xtask]
built_pr: 659
satisfies: [REQ-QUAL.VAP-001, REQ-QUAL.VAP-002]
covers: []
depends_on: [QUAL.VAP.TPL]
uses_artifacts: [CORE.REG.MAN, QUAL.IQOQ.RECORD]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-26
---

# QUAL.VAP.PROTO: IQ and OQ protocol templates and their command check

## Purpose

The IQ and OQ protocol templates a customer executes and signs: each step names the registry command it runs and the qualification-record field that holds its acceptance result, so the protocol and the product cannot drift. A CI check resolves every step against the Registry Manifest and the `qual-record/1` schema.

## Scope

**In:**
- Templates `iq-protocol` and `oq-protocol` in `vap-template/1` with a `steps` table: step ID, purpose, command ID, record field path, acceptance criterion text, result and signature fields
- Check: every command ID exists in the Registry Manifest and every record field path exists in `qual-record/1`

**Out** (and which node owns it instead):
- The commands and record (owned by `QUAL.IQOQ.*`)
- Template format and rendering (owned by `QUAL.VAP.TPL`)

## Requirements

- **REQ-QUAL.VAP.PROTO-001** Every step of the IQ and OQ protocols shall name a registry command ID and a qualification-record field path.
- **REQ-QUAL.VAP.PROTO-002** If a step names a command ID absent from the given Registry Manifest, or a field path absent from the `qual-record/1` schema, then the check shall fail and name the step.
- **REQ-QUAL.VAP.PROTO-003** The IQ protocol shall include steps for `validate.run_iq@1` and `validate.export_bundle@1`, and the OQ protocol steps for `validate.run_oq@1` and, when goldens are used, `validate.register_golden@1`.

## Acceptance criteria

- **AC-1** Given the shipped protocols, the manifest fixture `fixtures/qual/vap/proto/manifest.json` and the record schema, when `cargo xtask qual vap check-protocols --manifest manifest.json --record-schema qual-record-1.json` runs, then it exits 0.
- **AC-2** (error path) Given a protocol step naming `validate.run_iq@2`, then it exits 1 naming the step.
- **AC-3** (error path) Given a step naming field `checks.verdictt`, then it exits 1 naming the step.
- **AC-4** (error path) Given an OQ protocol without a `validate.run_oq@1` step, then it exits 1.

## Design

- **Registry command(s):** none; repository check `cargo xtask qual vap check-protocols --manifest <json> --record-schema <json>` (R0).
- **Data structures:** `Step { id, purpose, command, record_field, acceptance, result_field, signature_field }` within the template.
- **Algorithm:** load manifest command IDs into a BTreeSet; resolve field paths against the JSON Schema properties.
- **Errors:** `PROTO-COMMAND(step)`, `PROTO-FIELD(step)`, `PROTO-REQUIRED(command)`.
- **Determinism:** D0.
- **Performance budget:** under 1 s.
- **Crate:** `fid-xtask` (module `qual::vap::proto`), templates under `vap/templates/`.

## Test fixtures

`fixtures/qual/vap/proto/` with a synthetic Registry Manifest and the record schema; CC-BY-4.0.

## Build prompt

> Write `vap/templates/iq-protocol.json` and `oq-protocol.json` in format `vap-template/1`, each with a steps table (id, purpose, command ID, record field path, acceptance criterion in the project's own words, result and signature fields). IQ covers `validate.run_iq@1` (artefact, signature and kernel-manifest checks) and `validate.export_bundle@1`; OQ covers `validate.run_oq@1` and optional `validate.register_golden@1`. In `fid-xtask`, add `qual vap check-protocols --manifest <json> --record-schema <json>` that fails naming any step whose command is absent from the Registry Manifest or whose field path is absent from schema `qual-record/1`, or when required commands are missing. No panics, deny unwrap/expect. Tests: AC-1..AC-4 of spec node QUAL.VAP.PROTO.

## Open questions

- **Record schema location (for QUAL.IQOQ.RECORD).** No shipped JSON Schema for `qual-record/1` exists on main (QUAL.IQOQ.RECORD's Scope names one; `fid-qual` has the type only). This leaf ships it as the fixture `fixtures/qual/vap/proto/qual-record-1.json`, written by `make_fixtures.py`, and a test in `crates/fid-xtask/tests/qual_vap_proto.rs` pins its field paths to exactly the key paths `fid_qual::QualificationRecord::to_value` writes for a full IQ and OQ record. If QUAL.IQOQ.RECORD later publishes `schemas/qual/qual-record-1.schema.json`, CI should pass that file as `--record-schema` and the fixture can go.
- **Manifest read is structural (judgement call).** `check-protocols` reads only `format` (must be 1) and each command's `id` and `major` (a repeated pair is refused); it does not re-verify definitions or digests, which is `registry.show_manifest@1 --check`'s job against the catalogue in force. The fixture `manifest.json` is a reduced synthetic manifest (id, major, title) and says so.
- **Step table convention (pinned here).** A protocol's steps are one `vap-template/1` table field `steps` with columns `step`, `purpose`, `command`, `record-field`, `acceptance` (static) and `result`, `signature` (customer), in that order, and each row's `step` cell equals its row ID. `QUAL.VAP.GEN` should render it as is.
- **"When goldens are used" (REQ-003).** Read as: the OQ protocol needs a `validate.register_golden@1` step when any OQ step names a record field under `goldens`. The shipped OQ protocol has golden steps marked optional for the customer, so it always carries the registration step.
- **`validate.export_bundle@1` record field.** The bundle command writes no record field of its own; its IQ step names `verdict` (the bundled record's overall verdict). QUAL.IQOQ.BUNDLE may want a better anchor.

## Children

_Leaf: none._
