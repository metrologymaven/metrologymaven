---
id: ECO.REPO.TPL
title: Repository template
level: 3
parent: ECO.REPO
kind: module
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REPO-006]
covers: []
depends_on: [ECO.REPO.LAYERS, ECO.REPO.TOPO]
uses_artifacts: [ECO.RULES.BLOCK, ECO.RULES.SYNC, ECO.CI.WF, ECO.CI.GATES]
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# ECO.REPO.TPL: Repository template

## Purpose

The starting point for every new repository: standard files, workspace configuration and CI calls arranged so that the first commit of a new repository already passes the shared CI. It also covers promoting a library out of the monorepo into its own repository with the template applied.

## Scope

**In:**
- Template file set: licence files (MIT OR Apache-2.0 per D-001), `ATTRIBUTION.md`, `AGENTS.md` with the generated house-rules block, `CLAUDE.md` containing only `@AGENTS.md`, `CONTRIBUTING.md` pointer, workspace `Cargo.toml` with inherited lints, `layers.toml`, `repos.toml` entry, CI caller workflow pinned to a revision
- An instantiation script and its self-test (instantiate, run the shared CI locally, expect success)
- Library promotion procedure (history-preserving split, pins updated in consumers)

**Out** (and which node owns it instead):
- House-rules block content and sync (owned by `ECO.RULES`)
- Shared CI workflows (owned by `ECO.CI`)
- Layer map and pin rules (owned by `ECO.REPO.LAYERS`, `ECO.REPO.TOPO`)

## Requirements

- **REQ-ECO.REPO.TPL-001** When a repository is created from the template and nothing else is changed, every check in the shared required-check set shall pass on its first commit.
- **REQ-ECO.REPO.TPL-002** The template shall contain a `CLAUDE.md` whose only content is a reference to `AGENTS.md`, and an `AGENTS.md` containing the current generated house-rules block.
- **REQ-ECO.REPO.TPL-003** The template's self-test shall run in CI on every change to the template and on every change to the shared required-check set.
- **REQ-ECO.REPO.TPL-004** The library promotion procedure shall preserve the library's commit history and leave every consumer pinned to an immutable revision of the new repository.

## Interfaces

- **Offers:** `templates/repo/` with variants `code` and `spec`, and `python -m repo_template new` (FILES); the self-test job (SELFTEST); `PROMOTE.md` and `python -m repo_template promote` (PROMOTE).
- **Consumes (as files):** the house-rules block format and sync tool (`ECO.RULES.BLOCK`, `ECO.RULES.SYNC`); `required-checks.toml` and the reusable caller workflow (`ECO.CI.WF`); `ci-gates.toml` defaults (`ECO.CI.GATES`).
- **Code dependency:** `layers.toml` schema and checker (`ECO.REPO.LAYERS`) and `repos.toml` schema and pin checker (`ECO.REPO.TOPO`).

## Decomposition hints

FILES (template content and instantiation), SELFTEST (instantiate-and-check job), PROMOTE (library promotion procedure).

## Open questions

- D-005 (provisional egui): if a TypeScript MCP App host shim is ever needed, the template needs a variant for it; the FILES child keeps variants as data so one can be added.
- ECO.RULES and ECO.CI depend on ECO.REPO, so the children consume their outputs as files (`uses_artifacts`), never as code dependencies.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| ECO.REPO.TPL.FILES | Template file set and instantiation script (leaf) | 002 | none |
| ECO.REPO.TPL.SELFTEST | Template self-test job (leaf) | 001 003 | none |
| ECO.REPO.TPL.PROMOTE | Library promotion procedure and tool (leaf) | 004 | none |
