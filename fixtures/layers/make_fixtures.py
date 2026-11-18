#!/usr/bin/env python3
"""Generate the ECO.REPO.LAYERS fixture workspaces (CC-BY-4.0 project data).

Each subfolder is a tiny Cargo workspace with path dependencies only, plus its own
layers.toml and expected.txt (the expected findings, one per line).
Run:  python fixtures/layers/make_fixtures.py
"""

import shutil
from pathlib import Path

HERE = Path(__file__).parent
ORDER = 'order = ["num", "kernel", "model", "engine", "services", "viewer", "app", "tool"]'


def crate(ws: Path, folder: str, name: str, deps=(), dev=()):
    d = ws / folder
    (d / "src").mkdir(parents=True, exist_ok=True)
    lines = ["[package]", f'name = "{name}"', 'version = "0.0.0"', 'edition = "2021"', "publish = false", ""]
    if deps:
        lines += ["[dependencies]"] + [f'{n} = {{ path = "{p}" }}' for n, p in deps] + [""]
    if dev:
        lines += ["[dev-dependencies]"] + [f'{n} = {{ path = "{p}" }}' for n, p in dev] + [""]
    (d / "Cargo.toml").write_text("\n".join(lines), encoding="utf-8", newline="\n")
    (d / "src" / "lib.rs").write_text("//! Fixture crate for ECO.REPO.LAYERS.\n", encoding="utf-8", newline="\n")


def workspace(name, members, layers, expected, build):
    ws = HERE / name
    if ws.exists():
        shutil.rmtree(ws)
    ws.mkdir()
    (ws / "Cargo.toml").write_text(
        "[workspace]\nresolver = \"2\"\nmembers = [" + ", ".join(f'"{m}"' for m in members) + "]\n",
        encoding="utf-8", newline="\n")
    (ws / "layers.toml").write_text(ORDER + "\n\n[crates]\n" + "".join(f'"{c}" = "{l}"\n' for c, l in layers),
                                    encoding="utf-8", newline="\n")
    (ws / "expected.txt").write_text("".join(f"{e}\n" for e in expected), encoding="utf-8", newline="\n")
    build(ws)


workspace("ok", ["num", "krn", "appx"], [("fid-num", "num"), ("fid-krn", "kernel"), ("fid-app-x", "app")], [],
          lambda ws: (crate(ws, "num", "fid-num"), crate(ws, "krn", "fid-krn", [("fid-num", "../num")]),
                      crate(ws, "appx", "fid-app-x", [("fid-num", "../num"), ("fid-krn", "../krn")])))
workspace("upward", ["krn", "viz"], [("fid-krn", "kernel"), ("fid-viz", "viewer")],
          ["LAYER-UPWARD fid-krn -> fid-viz"],
          lambda ws: (crate(ws, "krn", "fid-krn", [("fid-viz", "../viz")]), crate(ws, "viz", "fid-viz")))
# The third-party-like crate lives outside the workspace folder so Cargo does not make it a member.
vendor = HERE / "_vendor"
if vendor.exists():
    shutil.rmtree(vendor)
crate(vendor, "shim", "shim", [("fid-svc", "../../transitive/svc")])
workspace("transitive", ["krn", "svc"], [("fid-krn", "kernel"), ("fid-svc", "services")],
          ["LAYER-UPWARD fid-krn -> shim -> fid-svc"],
          lambda ws: (crate(ws, "krn", "fid-krn", [("shim", "../../_vendor/shim")]), crate(ws, "svc", "fid-svc")))
workspace("unassigned", ["num", "new"], [("fid-num", "num")], ["LAYER-UNASSIGNED fid-new"],
          lambda ws: (crate(ws, "num", "fid-num"), crate(ws, "new", "fid-new")))
workspace("unknown", ["num"], [("fid-num", "num"), ("fid-gone", "num")], ["LAYER-UNKNOWN fid-gone"],
          lambda ws: crate(ws, "num", "fid-num"))
workspace("testsupport", ["krn", "testkit"], [("fid-krn", "kernel"), ("fid-testkit", "test-support")], [],
          lambda ws: (crate(ws, "krn", "fid-krn", dev=[("fid-testkit", "../testkit")]), crate(ws, "testkit", "fid-testkit")))
workspace("testsupport-normal", ["krn", "testkit"], [("fid-krn", "kernel"), ("fid-testkit", "test-support")],
          ["LAYER-TEST-SUPPORT fid-krn -> fid-testkit"],
          lambda ws: (crate(ws, "krn", "fid-krn", [("fid-testkit", "../testkit")]), crate(ws, "testkit", "fid-testkit")))
workspace("tool", ["appx", "xt"], [("fid-app-x", "app"), ("fid-xt", "tool")], ["LAYER-TOOL fid-app-x -> fid-xt"],
          lambda ws: (crate(ws, "appx", "fid-app-x", [("fid-xt", "../xt")]), crate(ws, "xt", "fid-xt")))
workspace("upward-dev", ["krn", "viz"], [("fid-krn", "kernel"), ("fid-viz", "viewer")],
          ["LAYER-UPWARD fid-krn -> fid-viz"],
          lambda ws: (crate(ws, "krn", "fid-krn", dev=[("fid-viz", "../viz")]), crate(ws, "viz", "fid-viz")))
workspace("testsupport-transit", ["krn", "testkit", "viz"],
          [("fid-krn", "kernel"), ("fid-testkit", "test-support"), ("fid-viz", "viewer")],
          ["LAYER-UPWARD fid-krn -> fid-testkit -> fid-viz"],
          lambda ws: (crate(ws, "krn", "fid-krn", dev=[("fid-testkit", "../testkit")]),
                      crate(ws, "testkit", "fid-testkit", [("fid-viz", "../viz")]), crate(ws, "viz", "fid-viz")))
print("ok")
