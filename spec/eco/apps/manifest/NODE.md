---
id: ECO.APPS.MANIFEST
title: App manifest schema, loader and status data
level: 3
parent: ECO.APPS
kind: module
status: split
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.APPS-002, REQ-ECO.APPS-006]
covers: []
depends_on: [CORE.REG, VIZ.SHELL]
uses_artifacts: [ECO.APPS.LIST]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.APPS.MANIFEST: App manifest schema, loader and status data

## Purpose

The declarative file that turns the shared engine into one focused app: which registry command namespaces it enables, which panels and workspace layout it shows, its default settings and report templates, its branding slot, and its declared release status and known gaps. This node owns the schema, the loader that the app binaries call at start-up, and the validation that keeps a manifest consistent with the registry and the app list. The content of each app's manifest belongs to the per-app nodes.

## Scope

**In:**
- Manifest schema (versioned), including status (`alpha`, `beta`, `stable`) and a known-gaps list
- Loader used by every app binary; fail-closed validation against the registry manifest, the VIZ panel catalogue and the app list
- Branding slot (name, icon, colours) left as placeholders until D-002 settles names
- Deterministic composition of several manifests, as the hook for a later suite host (D-004 option C)
- Machine-readable export of status and gaps for ECO.PARITY

**Out** (and which node owns it instead):
- Each app's manifest content (owned by `ECO.APPS.INSPECT`, `ECO.APPS.PROBE`, `ECO.APPS.NETWORK`, `ECO.APPS.REPORT`)
- The rule that app crates hold no domain logic and the cross-app command identity check (owned by `ECO.APPS.POLICY`)
- The app list (owned by `ECO.APPS.LIST`)
- Panel implementations and the panel catalogue (owned by `VIZ.SHELL`)
- Generating status pages from this data (owned by `ECO.PARITY`)

## Requirements

- **REQ-ECO.APPS.MANIFEST-001** Each manifest shall carry a schema version, the app `id` from the app list, and selections of registry command namespaces, panels, default settings, workspace layout and report templates, and nothing executable.
- **REQ-ECO.APPS.MANIFEST-002** If a manifest names a command namespace, panel, setting key or template that the registry manifest or panel catalogue does not define, then the loader shall refuse to start the app and report every unknown name.
- **REQ-ECO.APPS.MANIFEST-003** If a manifest's `id` is not in the app list, then manifest validation shall fail.
- **REQ-ECO.APPS.MANIFEST-004** Each manifest shall declare a status of `alpha`, `beta` or `stable` and a known-gaps list in which each entry is a seed ID or a free-text gap with an owning spec node ID.
- **REQ-ECO.APPS.MANIFEST-005** The manifest tooling shall export every app's status and known gaps as one JSON document whose bytes depend only on the manifests.
- **REQ-ECO.APPS.MANIFEST-006** When two or more manifests are composed, the result shall be the same for any input order, and conflicting default settings shall be reported rather than silently resolved.
- **REQ-ECO.APPS.MANIFEST-007** While D-002 is undecided, the branding slot shall accept only placeholder values marked internal, and release packaging shall be able to detect them.

## Interfaces

- **Offers:** manifest schema `app.toml` v1 and the loader in crate `fid-app-manifest` (ECO.APPS.MANIFEST.SCHEMA); `apps-status.json` v1 with per-app status, status scope, known gaps, enabled namespaces, effective commands and branding-placeholder flag (ECO.APPS.MANIFEST.EXPORT; read by ECO.PARITY.STATUS, ECO.APPS.POLICY, ECO.APPS.HANDOFF.RTRIP and ECO.REL); `compose()` for a future suite host (ECO.APPS.MANIFEST.COMPOSE).
- **Consumes:** CORE.REG registry manifest (namespaces, classes, definition digests); VIZ.SHELL panel and layout catalogue; CORE.STO.SET setting keys; RPT.MODEL template identifiers; `apps.toml` from ECO.APPS.LIST (artifact).

## Decomposition hints

SCHEMA (schema and validation), LOAD (loader and start-up refusal), EXPORT (status and gaps JSON), COMPOSE (deterministic composition for the suite hook).

## Open questions

- D-005 is accepted only provisionally (egui). Panels are referenced by catalogue ID so the manifest does not change if the toolkit is reopened, but the catalogue itself is VIZ.SHELL's and does not exist yet.
- File format settled at leaf level: TOML (`app.toml`), matching `apps.toml`.
- D-017 (accepted): an app reaches `stable` (1.0) only after every phase holding its core workflow is finished; earlier releases are `alpha` or `beta` labelled by scope. ECO.APPS.MANIFEST.SCHEMA adds a `status_scope` label required for non-stable status.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.APPS.MANIFEST.SCHEMA | Manifest schema, validation and start-up loader (leaf) | 001 002 003 004 007 | none |
| ECO.APPS.MANIFEST.EXPORT | Status, gaps and effective-command export (leaf) | 005 007 | none |
| ECO.APPS.MANIFEST.COMPOSE | Order-independent manifest composition (leaf) | 006 | none |
