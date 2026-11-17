---
id: ECO.REPO
title: Repository topology and build-time layering
level: 2
parent: ECO
kind: subsystem
status: open
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-ECO-003]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# ECO.REPO: Repository topology and build-time layering

## Purpose

Defines how the code is laid out across repositories and crates, and enforces the crate layering so a lower layer can never depend on a higher one. It covers the repository set (core, libs, apps, contrib, common-actions, spec), the crate layer map, the dependency allowlist for the kernel graph, the version-pinning rule between repos, and the repo template every new repo starts from.

## Scope

**In:**
- The repository set and what each holds; when a second repo is created
- The crate layer map (for example kernel, model, engine, services, viewer, app) as one machine-readable file
- The build-time layering check and its failure output
- The kernel-graph dependency allowlist (crate names and versions)
- Cross-repo pinning by tag or revision
- A repo template: standard files, workspace lints, CLAUDE.md containing only `@AGENTS.md`
- Promotion of a library to its own project

**Out** (and which node owns it instead):
- Running the checks in CI (owned by `ECO.CI`)
- The house-rules block inside each repo (owned by `ECO.RULES`)
- Which apps exist (owned by `ECO.APPS`)
- The licence allowlist of SPDX IDs (set by D-001, checked by `ECO.CI`)
- Numeric-profile lints inside the kernel (owned by `KRN`)

## Requirements

- **REQ-ECO.REPO-001** The project shall keep one machine-readable layer map that assigns every first-party crate to exactly one layer.
- **REQ-ECO.REPO-002** If a crate depends, directly or transitively, on a crate in a higher layer, then the layering check shall fail and name the offending dependency path.
- **REQ-ECO.REPO-003** If a crate in the kernel graph gains a dependency that is not on the kernel allowlist, then the layering check shall fail.
- **REQ-ECO.REPO-004** If a first-party crate is not assigned a layer, then the layering check shall fail.
- **REQ-ECO.REPO-005** Every repository shall pin each other first-party repository it uses to an immutable tag or revision, never to a moving branch.
- **REQ-ECO.REPO-006** The project shall provide a repo template from which a new repository passes the shared CI on its first commit.
- **REQ-ECO.REPO-007** The layering check shall run offline in under 60 s on the reference CI machine [unverified].

## Interfaces

- **Offers:** the repository map; the layer map file; the kernel allowlist file; the layering check (a repo tool invoked by ECO.CI, not a product registry command); the repo template.
- **Consumes:** the crate graph from the build tool.

## Decomposition hints

TOPO (repository set and pinning), LAYERS (layer map and check), ALLOW (kernel allowlist), TPL (repo template).

## Open questions

- D-005 decides whether any TypeScript host shims exist and which layer they sit in.
- Until there is more than one code repository, the "repository set" is this spec repo plus one code monorepo; when to split is a maintainer call.

## Children

_None yet: status `open`._
