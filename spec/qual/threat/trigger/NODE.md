---
id: QUAL.THREAT.TRIGGER
title: PR-level threat-model review trigger
level: 3
parent: QUAL.THREAT
kind: task
status: leaf
phase: P1a
size: S
owners: []
claimed_by:
satisfies: [REQ-QUAL.THREAT-005]
covers: []
depends_on: []
uses_artifacts: [CORE.REG.MAN, ECO.CI.GATES]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-17
---

# QUAL.THREAT.TRIGGER: PR-level threat-model review trigger

## Purpose

A CI check on every PR that detects architecture changes the threat model must answer for (a new command of class M1, S1, W3, X1 or A0, a new network listener, a new trust boundary) and requires either a threat-register change in the same PR or a linked issue labelled `threat-model`. It makes out-of-cycle reviews happen when the architecture changes rather than only at minor releases.

## Scope

**In:**
- Command trigger: compare the Registry Manifest of the PR's base and head; any command added, or whose class changed, to M1, S1, W3, X1 or A0 triggers
- Listener trigger: a change to `security/listeners.toml` (inventory of crates that open listening sockets), plus a scan of changed Rust files for a fixed list of listening-socket API names not covered by the inventory
- Boundary trigger: a change to `security/threat/boundaries.toml`
- Satisfaction: the PR changes `security/threat/register.toml`, or its body links an issue carrying the label `threat-model`
- Registration of the check in `ci-gates.toml` for the relevant globs

**Out** (and which node owns it instead):
- Register content and check (owned by `QUAL.THREAT.REG`)
- Generating the Registry Manifest (owned by `CORE.REG.MAN`)
- Selecting gates per PR (owned by `ECO.CI.GATES`)

## Requirements

- **REQ-QUAL.THREAT.TRIGGER-001** When a PR's head manifest contains a command of class M1, S1, W3, X1 or A0 that is absent from the base manifest or had another class there, the check shall report a `command` trigger naming the command.
- **REQ-QUAL.THREAT.TRIGGER-002** When a PR changes `security/listeners.toml` or `security/threat/boundaries.toml`, or adds a call to a listed listening-socket API in a crate not in the listener inventory, the check shall report a `listener` or `boundary` trigger.
- **REQ-QUAL.THREAT.TRIGGER-003** If a trigger fires and the PR neither changes `security/threat/register.toml` nor links an issue labelled `threat-model`, then the check shall fail and list every trigger.
- **REQ-QUAL.THREAT.TRIGGER-004** The check shall treat the PR body and issue labels as untrusted data and read them only from the CI-provided event file.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/qual/threat/trigger/new-x1/` (head manifest adds `demo.upload@1` of class X1, no register change, no link), when `cargo xtask qual threat trigger --base base-manifest.json --head head-manifest.json --changed changed.txt --event event.json` runs, then it exits 1 and names `demo.upload@1`.
- **AC-2** Given the same with `security/threat/register.toml` in `changed.txt`, then it exits 0.
- **AC-3** Given the same with an event linking issue 42 labelled `threat-model`, then it exits 0.
- **AC-4** Given `listener/` (a changed file in crate `fid-demo` calls `TcpListener::bind`, crate absent from `listeners.toml`), then it exits 1 with a `listener` trigger.
- **AC-5** Given `w2-only/` (new W2 command), then it exits 0 with no trigger.

## Design

- **Registry command(s):** none; repository tool `cargo xtask qual threat trigger --base <manifest> --head <manifest> --changed <file> --event <json> [--json]` (R0: reads only).
- **Data structures:** `Trigger { kind: Command|Listener|Boundary, subject }`; listener API list in `security/listener-apis.txt` (for example `TcpListener::bind`, `UdpSocket::bind`, `UnixListener::bind`) [heuristic].
- **Algorithm:** load both manifests (Registry Manifest JSON from `CORE.REG.MAN`), map command ID to class; diff; scan added lines of changed `.rs` files for listed API names and map files to crates by path prefix; read linked issues and labels from the event file.
- **Errors:** `TRIGGER-PARSE(file)`; exit 0 pass, 1 unanswered trigger, 2 bad input.
- **Determinism:** D0.
- **Performance budget:** under 2 s.
- **Crate:** `fid-xtask` (module `qual::threat`, file `trigger.rs`).

## Test fixtures

`fixtures/qual/threat/trigger/{new-x1,listener,w2-only}/` with synthetic manifests, change lists and event files; CC-BY-4.0.

## Build prompt

> In `fid-xtask` module `qual::threat`, add `qual threat trigger --base <manifest.json> --head <manifest.json> --changed <file> --event <json>`. Triggers: a command in head of class M1, S1, W3, X1 or A0 that is new or changed class versus base (Registry Manifest JSON, commands with id and side-effect class); a change to `security/listeners.toml` or `security/threat/boundaries.toml`; an added line in a changed `.rs` file using an API from `security/listener-apis.txt` in a crate not listed in `security/listeners.toml`. Satisfied if `security/threat/register.toml` is in the changed list or the event links an issue labelled `threat-model`. Treat PR body and labels as untrusted. Exit 0/1/2. Add a gate entry to `ci-gates.toml` (globs: manifest sources, `security/**`, `**/*.rs`). No panics, deny unwrap/expect. Tests: AC-1..AC-5 of spec node QUAL.THREAT.TRIGGER.

## Open questions

- The listening-socket API scan is a heuristic; a listener opened through a third-party crate is caught only through the inventory.
- `ECO.CI.GATES` must add this gate to its default `ci-gates.toml` (cross-domain).

## Children

_Leaf: none._
