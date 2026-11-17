---
id: IO.QIF.JSON.AGENT
title: QIF-JSON form markers, agent view and lossy-form rejection
level: 4
parent: IO.QIF.JSON
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-IO.QIF.JSON-005, REQ-IO.QIF.JSON-006]
covers: []
depends_on: [IO.QIF.JSON.PROJ]
side_effect: R1
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# IO.QIF.JSON.AGENT: QIF-JSON form markers, agent view and lossy-form rejection

## Purpose

QIF-JSON comes in three forms, marked by a top-level `$form`: `qif-json/lossless@1` (the persistence form), `qif-json/agent@1` (lossless plus `$derived` helper fields for agents and MCP) and `summary@1` (lossy, for display only). This leaf produces the agent view, strips `$derived` fields on re-import and refuses lossy forms, so an agent can never round-trip a summary into the document.

## Scope

**In:**
- `$form` marker rules for the three forms
- `agent_view(model) -> json`: the lossless projection plus `$derived` objects (resolved display names, unit symbols, parent paths, entity IDs)
- Stripping and counting `$derived` fields before an agent-form import
- Rejecting `summary@1` and unmarked documents on import

**Out:**
- The lossless rules (`IO.QIF.JSON.PROJ`); import and export commands (`IO.QIF.JSON.CMD`)
- Producing `summary@1` (MCP or RPT views; this leaf only rejects it)

## Requirements

- **REQ-IO.QIF.JSON.AGENT-001** Every QIF-JSON document the system writes shall carry a top-level `$form` naming one of `qif-json/lossless@1`, `qif-json/agent@1` or `summary@1`.
- **REQ-IO.QIF.JSON.AGENT-002** When an agent-form document is prepared for import, every `$derived` property at any depth shall be removed and the removed count returned for the import report.
- **REQ-IO.QIF.JSON.AGENT-003** After `$derived` fields are stripped, an agent view shall equal the lossless projection of the same model byte for byte under canonical serialisation.
- **REQ-IO.QIF.JSON.AGENT-004** If a document's `$form` is `summary@1`, missing or unknown, then preparation for import shall fail with `qif_json_lossy_form` (for `summary@1`) or `qif_json_invalid` (missing or unknown).
- **REQ-IO.QIF.JSON.AGENT-005** `$derived` values shall be treated as untrusted content on import and shall never influence the imported model.

## Acceptance criteria

- **AC-1** Given `fixtures/io/qif/json/agent/model.qif`, when `agent_view` is produced and stripped, then the canonical bytes equal those of the lossless projection.
- **AC-2** Given `fixtures/io/qif/json/agent/agent_edited.json` with 37 `$derived` fields, one of them altered, then preparation returns count 37 and the prepared document equals the unaltered lossless form.
- **AC-3** (error path) Given `fixtures/io/qif/json/agent/summary.json`, then preparation fails `qif_json_lossy_form`.
- **AC-4** (error path) Given a document without `$form`, then preparation fails `qif_json_invalid`.

## Design

- **Registry commands:** none (library used by `IO.QIF.JSON.CMD` and by MCP read tools).
- **Data structures:** `Form { Lossless, Agent, Summary }`; `Prepared { doc, derived_removed: u64 }`.
- **Algorithm:** iterative tree walk with a depth guard; `$derived` keys removed wherever they occur; form check first.
- **Errors:** `qif_json_lossy_form`, `qif_json_invalid`.
- **Determinism:** D0.
- **Performance budget:** linear in document size; 100 MB in < 1 s [unverified until measured].
- **Crate:** `fid-qif` (module `json::agent`).

## Test fixtures

`fixtures/io/qif/json/agent/` (CC-BY-4.0).

## Build prompt

> In crate `fid-qif`, add module `json::agent`: every written QIF-JSON document carries `$form` (`qif-json/lossless@1`, `qif-json/agent@1`, `summary@1`); `agent_view(&Model)` adds `$derived` objects (display names, unit symbols, parent paths, entity IDs) to the lossless projection; `prepare_import(json)` fails `qif_json_lossy_form` for `summary@1` and `qif_json_invalid` for missing or unknown forms, removes every `$derived` key at any depth (iterative walk with depth guard) and returns the count. Stripped agent view must equal the lossless form byte for byte. `$derived` content is untrusted and never read into the model. CONSTITUTION §8: no panics, no unwrap. Tests AC-1..AC-4.

## Open questions

- None.

## Children

_Leaf: none._
