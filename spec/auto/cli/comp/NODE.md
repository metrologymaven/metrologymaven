---
id: AUTO.CLI.COMP
title: Generated shell completions
level: 3
parent: AUTO.CLI
kind: task
status: leaf
phase: P0
size: S
owners: []
claimed_by:
satisfies: [REQ-AUTO.CLI-001, REQ-AUTO.CLI-009]
covers: []
depends_on: [AUTO.CLI.TREE]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.CLI.COMP: Generated shell completions

## Purpose

Completion scripts for bash, zsh, fish and PowerShell, generated from the same CLI tree, so completion offers exactly the registered commands and their parameters, with units for quantity parameters.

## Scope

**In:**
- Completion scripts for bash, zsh, fish, PowerShell, written by the CLI emitter
- Static completion of domains, verb-objects, flags and enum values; unit symbols for quantity parameters

**Out:**
- Dynamic completion of element names or handles from a live session (later, not specified)
- Installing completions into a user's shell (`ECO` packaging)

## Requirements

- **REQ-AUTO.CLI.COMP-001** The CLI emitter shall write one completion script per supported shell, generated only from the CLI tree for the current manifest digest.
- **REQ-AUTO.CLI.COMP-002** Each completion script shall offer every command path in the tree and no path absent from it.
- **REQ-AUTO.CLI.COMP-003** For a quantity parameter, completion shall offer the unit symbols the registry declares for its quantity kind.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json`, when surfaces are generated, then `completions/fid.bash`, `_fid`, `fid.fish` and `fid.ps1` exist and each starts with the generated header naming the manifest digest.
- **AC-2** Given the bash script loaded in a test shell, when completing `fid al`, then the candidates are exactly the fixture's domains starting with `al`.
- **AC-3** Given `fid demo set-offset --offset 3`, when completing, then bash offers the declared length units.
- **AC-4** Given two generations, then the scripts are byte-identical.

## Design

- **Registry command(s):** none.
- **Data structures:** none new; reads `CliTree`.
- **Algorithm:** template per shell over the ordered tree; quoting rules per shell.
- **Errors:** none at run time; generation errors propagate from the emitter.
- **Determinism:** D0.
- **Performance budget:** completion response < 100 ms in bash with 5,000 commands [unverified until measured].
- **Crate:** `fid-cli`

## Test fixtures

`fixtures/manifest/min-classes.json`, `fixtures/manifest/cli-args.json`; bash tests run on Linux CI only, PowerShell tests on Windows CI.

## Build prompt

> In crate `fid-cli`, extend the CLI surface emitter to write completion scripts for bash (`fid.bash`), zsh (`_fid`), fish (`fid.fish`) and PowerShell (`fid.ps1`) from `CliTree`, with the generated header. Complete domains, verb-objects, flags, enum values and, for quantity parameters, the registry-declared unit symbols. Acceptance: the four cases in the spec; byte-identical regeneration. Constraints: no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; no hand-written command names in templates.

## Open questions

- None.

## Children

_Leaf: none._
