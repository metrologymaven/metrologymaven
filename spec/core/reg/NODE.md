---
id: CORE.REG
title: Command registry and manifest
level: 2
parent: CORE
kind: subsystem
status: open
phase: P0
size: XL
owners: []
claimed_by:
satisfies: [REQ-CORE-001]
covers: []
depends_on: []
cli: [fid registry *]
mcp: [registry_*]
journal: []
updated: 2026-11-16
---

# CORE.REG: Command registry and manifest

## Purpose

The single source of truth for what commands exist. Every domain declares its commands here: ID and major version, typed parameters with quantity kinds and units, result type, side-effect class, MCP projection, determinism class, preconditions, declared effects, kernel algorithm IDs and exposure. A build step exports the declarations as one canonical, content-addressed **Registry Manifest**. Every generated surface (CLI, MCP, Python, GUI actions, docs) is built from that manifest and never from source. REG exists as its own unit because it is pure data and tooling: it executes nothing and stores no project state.

## Scope

**In:**
- The command definition model: fields, their types and validation rules
- Command ID and major-version rules (CONSTITUTION §3), and the derived MCP tool name
- Parameter and result schemas (JSON Schema 2020-12 plus project keywords for quantity, unit, entity reference and selector)
- Per-command policy fields: side-effect class, MCP projection, determinism class, idempotency, file-system effect, undo behaviour
- Preconditions as declarative predicates (expression language chosen at a lower level)
- Declared effects (event types a command may emit)
- Manifest export: canonical JSON, SHA-256 digest, per-tool definition digests
- Versioning and deprecation lifecycle (`active → deprecated → hidden → removed`) and old-major retention for replay
- Read-only registry queries: list, describe, manifest digest, diff between two manifests
- The surface-parity check input: the manifest is the reference set that every generator's output is compared against

**Out** (and which node owns it instead):
- Executing commands, checking principals and enforcing classes at run time (owned by `CORE.BUS`)
- Generating CLI, MCP servers, Python bindings and docs from the manifest (owned by `AUTO`)
- Recording the manifest digest in each session header (owned by `CORE.JRN`)
- The commands themselves (owned by each domain that registers them)
- Plugin command namespaces and packaging (owned by `ECO`)

## Requirements

- **REQ-CORE.REG-001** The registry shall reject at build time any command declaration that lacks an ID, major version, parameter schema, result schema, side-effect class, MCP projection or determinism class.
- **REQ-CORE.REG-002** The registry shall reject at build time any command ID that does not match `domain.verb_object@major`, or whose derived MCP tool name is not in `[a-z0-9_]` or exceeds 64 characters.
- **REQ-CORE.REG-003** The registry shall reject at build time any declaration whose MCP projection contradicts its side-effect class per CONSTITUTION §3 (for example an M1, S1 or A0 command projected as an exposed tool).
- **REQ-CORE.REG-004** Every numeric parameter or result field that represents a physical quantity shall declare a quantity kind and unit in its schema.
- **REQ-CORE.REG-005** When the manifest is exported, the registry shall serialise it as RFC 8785 canonical JSON and publish its SHA-256 digest.
- **REQ-CORE.REG-006** When the manifest is exported twice from the same declarations, the two exports shall be byte-identical.
- **REQ-CORE.REG-007** The manifest shall include one SHA-256 digest per command definition, so a consumer can pin a single command's definition.
- **REQ-CORE.REG-008** The registry shall keep every major version of a command that is not removed, and shall mark each with its lifecycle state.
- **REQ-CORE.REG-009** If a declaration change is not additive (a new optional parameter whose default preserves prior behaviour, or a new output field), then the registry build shall fail unless the major version is incremented.
- **REQ-CORE.REG-010** If a side-effect class or MCP projection of an existing major changes, then the registry build shall fail.
- **REQ-CORE.REG-011** When `fid registry diff` is given two manifests, the registry shall report added, removed, deprecated and changed commands, and shall flag every breaking change.
- **REQ-CORE.REG-012** All registry query commands shall be side-effect class R0.

## Interfaces

- **Offers:** `Registry::list`, `Registry::describe(id@major)`, `Registry::manifest() -> (bytes, digest)`, `Registry::diff(a, b)`; the command-declaration API (derive or attribute macros, chosen at leaf level) that every domain crate uses; the manifest JSON schema consumed by `AUTO` generators and by `CORE.BUS`.
- **Commands (R0):** `registry.list_commands@1`, `registry.describe_command@1`, `registry.show_manifest@1`, `registry.diff_manifests@1`. CLI `fid registry list-commands | describe-command | show-manifest | diff-manifests`; MCP `registry_*`.
- **Consumes:** nothing inside CORE.

## Decomposition hints

DEF (definition model and schema keywords), VAL (build-time validation rules), MAN (canonical export and digests), VER (versioning, deprecation, breaking-change detection), QRY (R0 query commands).

## Open questions

- Precondition language: CEL or a restricted Starlark (concept doc 03 open question 2). This can be decided inside the REG subtree; it does not need the maintainers unless it adds a dependency outside the licence allowlist (D-001).
- Should licence-tier entitlement tags exist in the manifest at all? That depends on D-001; leave the field out until D-001 is decided.

## Children

_None yet: status `open`._
