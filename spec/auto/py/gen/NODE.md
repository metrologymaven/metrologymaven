---
id: AUTO.PY.GEN
title: Generated fiducial Python package
level: 3
parent: AUTO.PY
kind: task
status: leaf
phase: P1b
size: M
owners: []
claimed_by:
satisfies: [REQ-AUTO.PY-001, REQ-AUTO.PY-002, REQ-AUTO.PY-008]
covers: [GOM.AUT.01]
depends_on: [AUTO.SURF.PROJ, AUTO.SURF.GEN, AUTO.SURF.ROOTS, AUTO.PY.PROTO]
side_effect: R0
determinism: D0
cli: []
mcp: []
journal: []
updated: 2026-11-16
---

# AUTO.PY.GEN: Generated fiducial Python package

## Purpose

The Python surface emitter. It writes the `fiducial` package: one typed function per command major, docstrings from the registry, a `Quantity` type that carries authored decimal text and unit unchanged, `RootHandle` for files, and the shared envelope as Python types. Every call checks that the package was generated from the same manifest digest as the connected session, so a script can never call a command with a stale signature.

## Scope

**In:**
- Python `SurfaceEmitter` (surface `py`): modules per domain, `.pyi` stubs, `surface-index.json`
- `Quantity`, `RootHandle`, `Envelope` and error classes
- Manifest-digest check on every call
- Refusing stubs for M1, S1 and A0 commands and for root-changing commands

**Out:**
- Transport to the host (`AUTO.PY.PROTO`)
- Script process management (`AUTO.PY.HOST`)
- Packaging and publishing the wheel (`ECO.REL`)

## Requirements

- **REQ-AUTO.PY.GEN-001** The emitter shall write one function per command major in module `fiducial.<domain>`, named as `AUTO.SURF.PROJ` derives it, with typed parameters and a docstring from the registry help text.
- **REQ-AUTO.PY.GEN-002** The package shall embed the manifest digest, and if it differs from the session's digest, then every call shall raise `VersionMismatchError` naming both digests and shall send nothing.
- **REQ-AUTO.PY.GEN-003** `Quantity` shall hold the authored decimal as a string and the unit symbol as typed, and the package shall never convert units or turn the decimal into a float before sending.
- **REQ-AUTO.PY.GEN-004** Parameters flagged `path_like` shall accept only `RootHandle` values, and passing a `str` or `pathlib.Path` shall raise `RawPathRefused` before sending.
- **REQ-AUTO.PY.GEN-005** For M1, S1, A0 and root-changing commands, the emitted function shall raise `NotAllowedFromScripts` naming the path the action requires, and shall send nothing.

## Acceptance criteria

- **AC-1** Given fixture `fixtures/manifest/min-classes.json`, when surfaces are generated, then `fiducial/align.py` defines `run_bestfit` with a docstring equal to the registry help text, and `mypy --strict` passes on a sample script using the stubs.
- **AC-2** Given a fake host session with a different manifest digest, when `fiducial.demo.show_thing()` is called, then `VersionMismatchError` is raised naming both digests and the fake host records no invocation.
- **AC-3** Given `fiducial.demo.set_offset(offset=Quantity("12.700", "mm"))` on the fake host, then the host receives `{"value": "12.700", "unit": "mm"}`.
- **AC-4** (error path) Given a `path_like` parameter passed `"C:/x.stl"`, then `RawPathRefused` is raised and nothing is sent.
- **AC-5** (error path) Given the M1 fixture command's function, when called, then `NotAllowedFromScripts` is raised naming the approval station.
- **AC-6** Given two generations, then the package trees are byte-identical.

## Design

- **Registry command(s):** none of its own.
- **Data structures:** Python: `Quantity(value: str, unit: str)`, `RootHandle(str)`, `Envelope` dataclass mirroring `AUTO.SURF.ENV`, exception hierarchy rooted at `FiducialError`.
- **Algorithm:** templates per module; functions call `fiducial._host.invoke(command_id, major, params)` from `AUTO.PY.PROTO`; digest compared on session start and cached.
- **Errors:** `VersionMismatchError`, `RawPathRefused`, `NotAllowedFromScripts`, plus mapping of envelope error codes to exceptions.
- **Determinism:** D0 for generated files.
- **Performance budget:** import of the full package < 300 ms on CPython 3.12 [unverified until measured].
- **Crate:** `fid-py-gen` (emitter); the generated Python lives in the `fiducial` package.

## Test fixtures

`fixtures/manifest/min-classes.json`, `fixtures/manifest/cli-args.json`; a fake host implementing the protocol in Python test code.

## Build prompt

> In crate `fid-py-gen`, implement the Python surface emitter (surface `py`). For each command major write a function in `fiducial/<domain>.py` (name from the projection, typed parameters, docstring from registry help text) plus `.pyi` stubs that pass `mypy --strict`; write `surface-index.json`. Provide `Quantity(value: str, unit: str)` that never converts or floats the value, `RootHandle` for `path_like` parameters (raise `RawPathRefused` for `str` or `Path`), and refusing stubs raising `NotAllowedFromScripts` for M1, S1, A0 and root-changing commands. Embed the manifest digest; on any call, if it differs from the session digest, raise `VersionMismatchError` naming both and send nothing. Calls go through `fiducial._host.invoke` (protocol in `AUTO.PY.PROTO`). Acceptance: the six cases in the spec. Constraints (Rust emitter): no panic, no `unsafe`, deny `unwrap`/`expect`/`panic`/`todo`/`unimplemented`/`unreachable`; generated Python has no hand-written per-command code.

## Open questions

- None.

## Children

_Leaf: none._
