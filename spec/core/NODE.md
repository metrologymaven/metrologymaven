---
id: CORE
title: Core platform: command registry, journal, project model
level: 1
parent: FID
kind: domain
status: split
phase: P0
size: XXL
owners: []
claimed_by:
satisfies: [REQ-FID-004, REQ-FID-005, REQ-FID-017, REQ-FID-021]
covers: [GOM.PRJ.*, PW.PRJ.*]
depends_on: []
cli: [fid project *, fid journal *, fid registry *, fid settings *, fid job *]
mcp: [project_*, journal_*, registry_*, settings_*, job_*]
journal: [project.*, journal.*, settings.*]
updated: 2026-11-16
---

# CORE: Core platform

## Purpose

The engine every app and surface stands on:
- the content-addressed **command registry**;
- the **command bus** that executes commands with side-effect classes and principals;
- the hash-chained **semantic journal**, with branches, undo and replay;
- the **project model** (Part → Revision → Plan → Run, stages, element tree);
- the **parametric dependency graph** that recomputes dependents when an input changes.

## Scope

**In:**
- Registry manifest and code generation hooks
- Command bus and principal handling
- Side-effect class enforcement
- Journal streams, branches and undo/redo
- Replay engine (all modes)
- Project container and stages
- Element tree and metadata/tags
- Dependency graph and recompute
- Settings, local storage (SQLite + content-addressed store)
- Jobs and cancellation
- Crash-safe writes

**Out:**
- CLI, MCP, Python and the control channel (AUTO)
- The presentation track and video (VIZ)
- Signing (TRUST)
- File formats (IO)
- Geometry types (GEO)

## Requirements

- **REQ-CORE-001** The registry shall be the single source of command definitions: ID, parameters with units, result type, side-effect class, MCP projection and determinism class. Its manifest shall be content-addressed.
- **REQ-CORE-002** When any surface invokes a command, the bus shall authenticate the principal from the channel (never from the request body) and enforce the side-effect class rules in CONSTITUTION §3.
- **REQ-CORE-003** When a W1 or W2 command commits, the system shall append one journal entry to an append-only, hash-chained journal using RFC 8785 canonical JSON and SHA-256.
- **REQ-CORE-004** The journal shall support branches, undo and redo. Agent W2 commands shall commit to an agent branch that only a human can promote.
- **REQ-CORE-005** The replay engine shall support the exact, re-targeted, verifier, dry-run and cross-version-diff modes, and report `IDENTICAL | EQUIVALENT | DIVERGENT | NOT_REPLAYABLE`.
- **REQ-CORE-006** The project model shall hold Part → Revision → Plan → Run, with stages for multiple parts or measurements, and an element tree with tags and metadata.
- **REQ-CORE-007** When an input element changes (for example, the measured mesh is replaced), the dependency graph shall recompute every dependent element and journal the recompute as one entry.
- **REQ-CORE-008** If the process crashes or the GPU device is lost, then no committed command shall be lost and the session shall resume from the journal.
- **REQ-CORE-009** The core shall run with no network access and no account, and shall not send telemetry unless the user opts in.
- **REQ-CORE-010** Long-running commands shall run as cancellable jobs with progress reporting.

## Interfaces

- **Offers:** `Registry` (list, describe, manifest digest), `Bus::execute(command, params, principal)`, `Journal` (append, branch, promote_request, replay), `Project` API, `DepGraph`, `Jobs`.
- **Consumes:** nothing above it. Every other domain registers its commands here.

## Decomposition hints

REG (registry and manifest), BUS (execution, principals, classes), JRN (journal, branches, undo), RPL (replay), PRJ (project model and stages), DEP (dependency graph), STO (storage, atomic writes, settings), JOB (jobs).

## Open questions

- Should the dependency graph live in CORE or in a separate "document model" subsystem shared with GEO? Recommendation: CORE.DEP, with element types provided by their domains.

## Split rationale

The dividing principle is **one responsibility per stage of a command's life**: a command is *described* (REG), *executed under policy* (BUS), possibly *run as a job* (JOB), *recorded* (JRN) on *durable storage* (STO), and later *reconstructed or re-executed* (RPL). The document it changes is the *project model* (PRJ), and the *dependency graph* (DEP) keeps that document consistent when inputs change. This follows the parent's decomposition hints one for one, and each child maps to a crate-level boundary that the no-side-doors rules need anyway (only BUS reaches mutable state; only STO touches the disk).

Shared responsibilities are resolved by ownership plus `depends_on`:
- REQ-CORE-004 (agent branches): BUS routes agent commits to agent branches; JRN owns branches and the human-only promotion.
- REQ-CORE-008 (crash safety): STO guarantees atomic durable commits; JRN guarantees an acknowledged append is durable; RPL restores state on restart.
- REQ-CORE-009 (offline, no telemetry): STO owns the no-account operation and the telemetry switch; BUS guarantees no network use outside X1, I1 or D1 commands.

Alternatives considered:
- **Fold replay into the journal (JRN+RPL).** Rejected: replay depends on the bus, the registry and kernel versions, while the journal must not; merging creates a cycle and an XXL node.
- **Fold DEP into PRJ.** Rejected: recompute is an algorithm with its own determinism, cancellation and journaling rules; keeping it apart also leaves room for the parent's open question about a shared document-model subsystem.
- **Fold JOB into BUS.** Rejected: job scheduling and cancellation are testable on their own and are shared with recompute and every surface's progress model.
- **A separate "settings" child.** Rejected as too small; settings live in STO with the privacy defaults.

The target's `cli:`, `mcp:` and `journal:` lists gained `fid settings *` / `settings_*` / `settings.*` and `fid job *` / `job_*`, because settings and jobs are in the parent's Scope but had no command namespace.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| CORE.REG | Command registry and manifest | 001 | none |
| CORE.BUS | Command bus: execution, principals and side-effect enforcement | 002 004 009 | none |
| CORE.JRN | Semantic journal: hash chain, streams, branches and undo | 003 004 008 | GOM.PRJ.06, PW.PRJ.04 |
| CORE.RPL | Replay engine and state reconstruction | 005 008 | none |
| CORE.PRJ | Project model: container, stages and element tree | 006 | GOM.PRJ.01-04, PW.PRJ.01-03, PW.PRJ.05 |
| CORE.DEP | Parametric dependency graph and recompute | 007 | GOM.PRJ.05, GOM.PRJ.07 |
| CORE.STO | Local storage, crash-safe writes and settings | 008 009 | none |
| CORE.JOB | Jobs: long-running commands, progress and cancellation | 010 | none |
