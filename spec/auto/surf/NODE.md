---
id: AUTO.SURF
title: Surface generation and shared surface contract
level: 2
parent: AUTO
kind: subsystem
status: split
phase: P0
size: L
owners: []
claimed_by:
satisfies: [REQ-AUTO-001, REQ-AUTO-005, REQ-AUTO-010]
covers: []
depends_on: [CORE]
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.SURF: Surface generation and shared surface contract

## Purpose

The single pipeline that turns the CORE registry manifest into surface descriptions (CLI command tree, MCP tool set, Python bindings, command docs), the CI check that every command has every surface and every surface has a command, and the runtime contract every surface shares: result and error envelope, progress, cancellation, honest timeouts, and file access through approved root handles. It exists separately so that CLI, MCP, Python and the control channel cannot drift apart: they consume one projection, not four hand-kept copies.

## Scope

**In:**
- Surface projection model: one derived description per registered command (names per CONSTITUTION §3, parameters with units, result type, side-effect class, MCP projection, determinism class)
- Generator entry points that the surface children call; generated-file headers marking them "do not edit"
- Command reference docs generated from the manifest (the docs portal publishes them; OUTR)
- CI surface-completeness check (command without surface, surface without command, stale generated output)
- Shared result and error envelope, including the machine-readable error code set
- Progress, cancellation and timeout semantics shared by all surfaces (over CORE jobs)
- File-access policy: read roots, write roots, opaque handles, refusal of raw paths from automation principals

**Out** (and which node owns it instead):
- The manifest, bus, principals and jobs themselves (`CORE`)
- CLI argument syntax, exit codes, `--json` rendering (`AUTO.CLI`)
- MCP tool listing, disclosure and transports (`AUTO.MCP`)
- Python package layout and script host (`AUTO.PY`)
- Control-channel wire protocol and authentication (`AUTO.CTRL`)
- Configuring the roots themselves: an A0 command (`CORE`; the GUI settings page is `VIZ`)

## Requirements

- **REQ-AUTO.SURF-001** The generator shall derive the CLI, MCP, Python and docs descriptions of every command only from the content-addressed registry manifest, and each generated artifact shall record the manifest digest it was generated from.
- **REQ-AUTO.SURF-002** When CI runs, the surface-completeness check shall fail if any registered command lacks a CLI, Python or docs entry, lacks an MCP entry while its class is projectable, or if any generated surface entry names a command absent from the manifest.
- **REQ-AUTO.SURF-003** If a generated artifact differs from a fresh generation against the same manifest digest, then the CI check shall fail and name the stale file.
- **REQ-AUTO.SURF-004** The generator shall be deterministic: two generations from the same manifest digest shall produce byte-identical output.
- **REQ-AUTO.SURF-005** Every surface shall return results and errors in one shared envelope carrying command ID, outcome, typed error code, journal entry hash when committed, and job ID when asynchronous.
- **REQ-AUTO.SURF-006** When a command runs longer than 1 s [unverified], every surface shall be able to observe its progress and request its cancellation through the CORE job it runs as.
- **REQ-AUTO.SURF-007** If a surface gives up waiting for a reply, then it shall report exactly one of `NOT_RUN` (the bus never accepted the command, safe to resend) or `NO_REPLY` (the command may have committed), and shall never report `NO_REPLY` as `NOT_RUN`.
- **REQ-AUTO.SURF-008** Automation principals (agents, scripts, control-channel clients) shall name files only through opaque handles resolved against the configured read and write roots, and any raw path parameter from such a principal shall be refused with a typed error before the bus executes anything.
- **REQ-AUTO.SURF-009** If a handle resolves, after symlink and junction resolution, to a location outside its root, then the resolution shall fail closed and nothing shall be read or written.
- **REQ-AUTO.SURF-010** No surface shall offer any command that changes the read or write roots to an automation principal.

## Interfaces

- **Offers:** `SurfaceProjection` (per-command derived description), generator entry points used by `AUTO.CLI`, `AUTO.MCP`, `AUTO.PY`, `AUTO.CTRL`; the shared `Envelope` and error-code set; `RootHandle` resolution; the CI check `surface-completeness`; generated command reference pages.
- **Consumes:** CORE registry manifest and digest, CORE bus (principals), CORE jobs (progress, cancel), CORE settings (root configuration, read-only here).

## Decomposition hints

PROJ (projection model and naming), GEN (generator and stale check), DOCS (reference pages), ENV (envelope and error codes), JOBS (progress, cancel, honest timeouts), ROOTS (handles and path refusal).

## Open questions

- Whether a human principal at the CLI may pass raw paths (likely yes, since it is the user's own shell) while agents never may. This child assumes yes for human-run CLI and no for every automation principal.
- The raw-path question above is now D-012 (CORE.BUS-002 assigns `script` to every CLI caller). `AUTO.SURF.ROOTS` keys its rule on principal kind, so it is correct under any outcome.

## Split rationale

The split follows the life of a command's surface description: derive it (PROJ), write it (GEN), prove it is complete and fresh (CHECK), document it (DOCS), and then the three runtime contracts every surface shares at call time: the result shape (ENV), job and timeout behaviour (JOBS), and file access (ROOTS). Build-time children live in the xtask crate `fid-surface-gen`; runtime children live in `fid-surface`, which each surface links.

Alternatives considered:
- **One generator child per surface here.** Rejected: emitters belong to the surface nodes (`AUTO.CLI.TREE`, `AUTO.MCP.PROJ`, `AUTO.PY.GEN`); SURF owns only the shared framework, so a surface cannot redefine names.
- **CHECK inside GEN.** Rejected: the check must regenerate and compare independently of a developer's local generation, and it is the gate CI calls; keeping it separate keeps GEN a pure writer.
- **JOBS inside ENV.** Rejected: ENV is a pure data contract (D0); JOBS is a timing-dependent state machine (N) with its own property tests.

## Children

| ID | Title | Satisfies | Covers |
|---|---|---|---|
| AUTO.SURF.PROJ | Surface projection model and name derivation (leaf) | 001 | none |
| AUTO.SURF.GEN | Surface generator framework and digest stamping (leaf) | 001 004 | none |
| AUTO.SURF.CHECK | CI surface-completeness and staleness check (leaf) | 002 003 | none |
| AUTO.SURF.DOCS | Generated command reference pages | 001 002 | none |
| AUTO.SURF.ENV | Shared result envelope and error-code set (leaf) | 005 | none |
| AUTO.SURF.JOBS | Progress, cancellation and honest timeouts for surfaces (leaf) | 006 007 | none |
| AUTO.SURF.ROOTS | Root handles and raw-path refusal (leaf) | 008 009 010 | none |
| AUTO.SURF.INPROC | In-process bus transport and bus-error envelopes (leaf) | 005 007 | none |
| AUTO.SURF.COMPOSE | Composition root: handlers, project buses and the fid entry point (leaf) | 008 | none |
