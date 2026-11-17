---
id: D-012
title: Principal kind of a human at the CLI or starting a Python script
status: open
raised_by: AUTO.SURF and AUTO.PY level-3 expansion
blocks: [AUTO.PY.RUN]
decide_by: before AUTO.PY.RUN or AUTO.CLI.GATE is built
updated: 2026-11-16
---

# D-012: Principal kind of a human at the CLI or starting a Python script

## Question

When a person runs `fid ...` in their own terminal, or starts a Python script from the GUI or CLI, is the bus principal `human` (that person) or `script`?

## Context

Three accepted level-2 nodes disagree:

- **REQ-CORE.BUS-002**: every CLI, Python or headless caller gets principal kind `script`, never `human`.
- **REQ-AUTO.PY-005**: a script started by a human from the GUI or CLI runs under that human's principal.
- **AUTO.SURF** (open question) assumes a human at the CLI may pass raw file paths, while automation principals must use root handles (REQ-AUTO.SURF-008).

The answer decides whether CLI users can import from arbitrary paths, whether a human-run script commits to the working branch or to a script branch, whether it may perform W3 (release) directly, and who can cancel its jobs (REQ-CORE.JOB-008). The CLI cannot prove a human is present: any process running as the OS user can call `fid`, including an agent with shell access. That is why CORE chose `script`.

Affected nodes: `AUTO.PY.RUN` (blocked), `AUTO.CLI.GATE`, `AUTO.SURF.ROOTS` (keyed on principal kind, works either way), `CORE.BUS`.

## Options

| Option | For | Against |
|---|---|---|
| A. Always `script` (CORE.BUS-002 as written). Raw paths from the CLI are refused; users import through roots or an interactive GUI picker. Human-started scripts run as `script` with the human recorded as `on_behalf_of` | One rule; an agent with shell access gains nothing by calling `fid` | CLI ergonomics suffer; REQ-AUTO.PY-005 must change |
| B. `human` when the CLI is attached to an interactive terminal (TTY) and `script` otherwise | Good ergonomics for people | A TTY is easy to fake; agents running in a terminal emulator would be `human` |
| C. `script` by default, with an explicit per-session human confirmation (GUI prompt or OS-level user presence) that upgrades a CLI session or script run to `human` for its lifetime | Keeps the safe default and gives people a real path | Needs a confirmation UI; more moving parts |

## Recommendation

C. Keep CORE.BUS-002 as the default; add a confirmation step in the running app (or OS user-presence check when headless) to upgrade a session; amend REQ-AUTO.PY-005 to say "under the human's principal after confirmation". Raw paths remain refused for `script`.

## Decision

_Open._ When decided: update REQ-CORE.BUS-002 or REQ-AUTO.PY-005 accordingly, unblock `AUTO.PY.RUN`, and resolve the open question in `AUTO.SURF`.
