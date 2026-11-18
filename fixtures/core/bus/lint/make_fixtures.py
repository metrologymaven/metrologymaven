#!/usr/bin/env python3
"""Generate the CORE.BUS.LINT fixture workspaces (CC-BY-4.0 project data).

Each subfolder is a tiny Cargo workspace with path dependencies only and empty crate
bodies, plus lint-allowlist.toml, storage-writers.toml (the CORE.STO.LAYOUT file shape)
and expected.txt: the findings of `cargo xtask lint-crate-graph`, one per line, labelled
by hand from the rules of spec/core/bus/lint/NODE.md (not produced by the Rust code).

The SQLite driver is a stand-in crate named `rusqlite` under _vendor/, outside every
fixture workspace, so it is a non-member dependency (like a crates.io crate) and the
fixtures resolve offline.

Run:  python fixtures/core/bus/lint/make_fixtures.py
"""

import shutil
from pathlib import Path

HERE = Path(__file__).parent

ALLOWLIST = """\
# Fixture allowlist for CORE.BUS.LINT.
[mutable_state]
crate = "fid-state"
allowed = ["fid-bus", "fid-store"]

[sqlite]
drivers = ["libsqlite3-sys", "rusqlite"]

[surfaces]
crates = ["cli", "fid-viewer"]
bus_api = "fid-bus-api"

[core]
crates = ["fid-bus", "fid-bus-api", "fid-state", "fid-store"]
"""

WRITERS = """\
# Crates allowed to write project files (shape published by CORE.STO.LAYOUT).
[storage_writers]
allowed = ["fid-store"]
"""


def write(path: Path, text: str):
    path.write_text(text, encoding="utf-8", newline="\n")


def crate(ws: Path, folder: str, name: str, deps=(), dev=(), opt=()):
    d = ws / folder
    (d / "src").mkdir(parents=True, exist_ok=True)
    lines = ["[package]", f'name = "{name}"', 'version = "0.0.0"', 'edition = "2021"', "publish = false", ""]
    if deps or opt:
        lines += (["[dependencies]"] + [f'{n} = {{ path = "{p}" }}' for n, p in deps]
                  + [f'{n} = {{ path = "{p}", optional = true }}' for n, p in opt] + [""])
    if dev:
        lines += ["[dev-dependencies]"] + [f'{n} = {{ path = "{p}" }}' for n, p in dev] + [""]
    write(d / "Cargo.toml", "\n".join(lines))
    write(d / "src" / "lib.rs", "//! Fixture crate for CORE.BUS.LINT.\n")


RUSQLITE = ("rusqlite", "../../_vendor/rusqlite")


def base(extra_cli=()):
    """The allowed architecture: only fid-bus and fid-store reach fid-state, only fid-store
    reaches the SQLite driver, and the surface `cli` uses the bus API and a kernel crate.
    fid-kernel's dev-dependency on fid-state is test-only and does not ship."""
    return [
        ("state", "fid-state", [], []),
        ("store", "fid-store", [("fid-state", "../state"), RUSQLITE], []),
        ("api", "fid-bus-api", [], []),
        ("bus", "fid-bus",
         [("fid-bus-api", "../api"), ("fid-kernel", "../kernel"), ("fid-state", "../state"), ("fid-store", "../store")],
         []),
        ("kernel", "fid-kernel", [], [("fid-state", "../state")]),
        ("cli", "cli", [("fid-bus-api", "../api"), ("fid-kernel", "../kernel")] + list(extra_cli), []),
    ]


def workspace(name, crates, expected, optional=None):
    """`optional`: (folder, [(dep, path)]) optional dependencies added to that crate."""
    ws = HERE / name
    if ws.exists():
        shutil.rmtree(ws)
    ws.mkdir(parents=True)
    write(ws / "Cargo.toml",
          "[workspace]\nresolver = \"2\"\nmembers = [" + ", ".join(f'"{c[0]}"' for c in crates) + "]\n")
    write(ws / "lint-allowlist.toml", ALLOWLIST)
    write(ws / "storage-writers.toml", WRITERS)
    write(ws / "expected.txt", "".join(f"{e}\n" for e in expected))
    for folder, cname, deps, dev in crates:
        opt = optional[1] if optional and optional[0] == folder else ()
        crate(ws, folder, cname, deps, dev, opt)


vendor = HERE / "_vendor"
if vendor.exists():
    shutil.rmtree(vendor)
crate(vendor, "rusqlite", "rusqlite")

# AC-1: allowed edges only.
workspace("clean", base(), [])

# AC-2: the surface reaches fid-state through an intermediate crate. helper is reported
# too, since it is itself a crate outside the allowlist that reaches fid-state. fid-state is
# a CORE crate, so the surface rule (transitive, REQ-004) reports cli as well.
workspace("side_door", base([("helper", "../helper")]) + [("helper", "helper", [("fid-state", "../state")], [])],
          ["cli: mutable_state cli -> helper -> fid-state",
           "cli: surface_dep cli -> helper -> fid-state",
           "helper: mutable_state helper -> fid-state"])

# AC-3: a viewer crate depends on the SQLite driver.
workspace("sqlite_leak",
          base() + [("viewer", "fid-viewer", [("fid-bus-api", "../api"), RUSQLITE], [])],
          ["fid-viewer: sqlite_driver fid-viewer -> rusqlite"])

# REQ-004: a surface depends directly on a CORE crate other than the bus API. The path
# cli -> fid-bus -> fid-state passes through an allowed crate, so it is not a side door.
workspace("surface_core", base([("fid-bus", "../bus")]), ["cli: surface_dep cli -> fid-bus"])

# Review of #62: a surface reaches the store through a non-CORE helper. The store is a door
# for the state and SQLite rules, so only the (transitive) surface rule catches it.
workspace("surface_store", base([("helper", "../helper")]) + [("helper", "helper", [("fid-store", "../store")], [])],
          ["cli: surface_dep cli -> helper -> fid-store"])

# Optional dependencies count: the check resolves with --all-features.
workspace("optional_dep", base() + [("viewer", "fid-viewer", [("fid-bus-api", "../api")], [])],
          ["fid-viewer: sqlite_driver fid-viewer -> rusqlite"], optional=("viewer", [RUSQLITE]))

print("ok")
