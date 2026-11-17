---
id: ECO.REPO.TPL
title: Repository template
level: 3
parent: ECO.REPO
kind: module
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO.REPO-006]
covers: []
depends_on: [ECO.REPO.LAYERS, ECO.REPO.TOPO]
cli: []
mcp: []
journal: []
updated: 2026-11-16
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

- **Offers:** template repository; instantiation script; promotion procedure.
- **Consumes:** ECO.RULES generated block and sync tool; ECO.CI required-check set and caller workflow; ECO.REPO.LAYERS and ECO.REPO.TOPO files. (ECO.RULES and ECO.CI depend on ECO.REPO, so these are consumed as files, not declared dependencies.)

## Decomposition hints

FILES (template content), SELFTEST (instantiate-and-check job), PROMOTE (library promotion procedure).

## Open questions

- D-005 (provisional egui): if a TypeScript MCP App host shim is ever needed, the template needs a variant for it.

## Children

_None yet: status `open`._
