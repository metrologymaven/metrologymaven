#!/usr/bin/env python3
"""Generate the ECO.REPO.ALLOW fixture workspaces (CC-BY-4.0 project data).

Each subfolder is a tiny Cargo workspace with path dependencies only, plus its own
layers.toml, kernel-allowlist.toml and expected.txt. Crates under _vendor/ stand in for
third-party crates: they live outside the fixture workspace folder, so Cargo does not make
them members. expected.txt is hand-labelled from the rules of spec node ECO.REPO.ALLOW (one
line per warning, then one line per finding, each group sorted); it is never produced by
running the Rust check.

Run:  python fixtures/allow/make_fixtures.py
"""

import shutil
from pathlib import Path

HERE = Path(__file__).parent
ORDER = 'order = ["num", "kernel", "model", "engine", "services", "viewer", "app", "tool"]'
VENDOR = HERE / "_vendor"


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


def crate(d: Path, name: str, version="0.0.0", deps=(), build=(), dev=()):
    lines = ["[package]", f'name = "{name}"', f'version = "{version}"', 'edition = "2021"', "publish = false", ""]
    for section, items in (("dependencies", deps), ("build-dependencies", build), ("dev-dependencies", dev)):
        if items:
            lines += [f"[{section}]"] + [f'{n} = {{ path = "{p}" }}' for n, p in items] + [""]
    write(d / "Cargo.toml", "\n".join(lines))
    write(d / "src" / "lib.rs", "//! Fixture crate for ECO.REPO.ALLOW.\n")


def entry(name, version, reason="fixture", reviewer="metrologymaven"):
    out = f'[[allow]]\nname = "{name}"\nversion = "{version}"\n'
    if reason is not None:
        out += f'reason = "{reason}"\n'
    if reviewer is not None:
        out += f'reviewer = "{reviewer}"\n'
    return out


def workspace(name, members, layers, allow, expected):
    ws = HERE / name
    if ws.exists():
        shutil.rmtree(ws)
    ws.mkdir()
    write(ws / "Cargo.toml",
          '[workspace]\nresolver = "2"\nmembers = [' + ", ".join(f'"{m}"' for m in members) + "]\n")
    write(ws / "layers.toml", ORDER + "\n\n[crates]\n" + "".join(f'"{c}" = "{l}"\n' for c, l in layers))
    write(ws / "kernel-allowlist.toml", "# Fixture allowlist for ECO.REPO.ALLOW.\n" + "\n".join(allow))
    write(ws / "expected.txt", "".join(f"{e}\n" for e in expected))
    return ws


if VENDOR.exists():
    shutil.rmtree(VENDOR)

# AC-1: fid-krn -> ext-a 1.2.0, allowlisted as ^1.2.
ws = workspace("ok", ["krn"], [("fid-krn", "kernel")], [entry("ext-a", "^1.2")], [])
crate(ws / "krn", "fid-krn", deps=[("ext-a", "../../_vendor/ok/ext-a")])
crate(VENDOR / "ok" / "ext-a", "ext-a", "1.2.0")

# AC-2: ext-a depends on ext-b, which is not listed.
ws = workspace("unlisted", ["krn"], [("fid-krn", "kernel")], [entry("ext-a", "^1.2")],
               ["ALLOW-UNLISTED ext-b via fid-krn -> ext-a -> ext-b"])
crate(ws / "krn", "fid-krn", deps=[("ext-a", "../../_vendor/unlisted/ext-a")])
crate(VENDOR / "unlisted" / "ext-a", "ext-a", "1.2.0", deps=[("ext-b", "../ext-b")])
crate(VENDOR / "unlisted" / "ext-b", "ext-b", "0.1.0")

# AC-3: ext-a 2.0.0 is resolved, the entry says ^1.2.
ws = workspace("version", ["krn"], [("fid-krn", "kernel")], [entry("ext-a", "^1.2")],
               ["ALLOW-VERSION ext-a 2.0.0 ^1.2"])
crate(ws / "krn", "fid-krn", deps=[("ext-a", "../../_vendor/version/ext-a")])
crate(VENDOR / "version" / "ext-a", "ext-a", "2.0.0")

# AC-4: an entry without reviewer.
ws = workspace("schema", ["krn"], [("fid-krn", "kernel")], [entry("ext-a", "^1.2", reviewer=None)],
               ["ALLOW-SCHEMA entry 1: missing reviewer"])
crate(ws / "krn", "fid-krn", deps=[("ext-a", "../../_vendor/schema/ext-a")])
crate(VENDOR / "schema" / "ext-a", "ext-a", "1.2.0")

# AC-5: an entry for ext-z that no kernel crate reaches.
ws = workspace("unused", ["krn"], [("fid-krn", "kernel")], [entry("ext-a", "^1.2"), entry("ext-z", "^1")],
               ["warn ALLOW-UNUSED ext-z"])
crate(ws / "krn", "fid-krn", deps=[("ext-a", "../../_vendor/unused/ext-a")])
crate(VENDOR / "unused" / "ext-a", "ext-a", "1.2.0")

# AC-6: a viewer-layer crate depends on an unlisted crate; only the kernel graph is checked.
ws = workspace("viewer", ["krn", "viz"], [("fid-krn", "kernel"), ("fid-viz", "viewer")], [], [])
crate(ws / "krn", "fid-krn")
crate(ws / "viz", "fid-viz", deps=[("ext-v", "../../_vendor/viewer/ext-v")])
crate(VENDOR / "viewer" / "ext-v", "ext-v", "3.0.0")

# REQ-001 edge kinds: a build-dependency of a num-layer crate counts and is reached through
# the first-party crate; a dev-dependency does not count.
ws = workspace("edges", ["num", "krn"], [("fid-num", "num"), ("fid-krn", "kernel")], [],
               ["ALLOW-UNLISTED ext-c via fid-krn -> fid-num -> ext-c"])
crate(ws / "num", "fid-num", build=[("ext-c", "../../_vendor/edges/ext-c")])
crate(ws / "krn", "fid-krn", deps=[("fid-num", "../num")], dev=[("ext-d", "../../_vendor/edges/ext-d")])
crate(VENDOR / "edges" / "ext-c", "ext-c", "1.0.0")
crate(VENDOR / "edges" / "ext-d", "ext-d", "1.0.0")

print("ok")
