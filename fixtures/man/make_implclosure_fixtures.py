#!/usr/bin/env python3
"""Generate the KRN.MAN.MANI.IMPL fixture `fixtures/man/implclosure-ws-v1` (CC-BY-4.0 project data).

Independent reference for the Rust `fid_manifest_gen::implclosure` module. Nothing here runs or
imports the Rust code: the crate closures come from a breadth-first walk over the `cargo
metadata` file written below, file digests and implementation digests from `hashlib`, and
canonical JSON from Python's `json` module (sorted keys, no whitespace; every value is an ASCII
string, where that equals RFC 8785). The closures are also hand-labelled (HAND_CLOSURES) and the
script refuses to write anything if its own walk disagrees with the labels.

The toy workspace (5 crates, 3 algorithms):

    knum   0.1.0  features [std]           src/ with a nested module and a binary file; tests/
    kgeo   0.2.0  -> knum (normal)         build.rs at the crate root; CRLF source; benches/
    kfit   0.3.0  -> kgeo, knum            features [default, fast]; examples/;
                  (knum: normal + build)   src/a-b.rs, src/a.rs, src/a/b.rs (byte-order sort)
    kstat  0.1.0  -> knum (normal)         dev-dependency on kfit (not in the kernel graph);
                  -> vext (build, wasm32)  a build-only, platform-gated edge (in the graph)
    vext   1.4.2  vendored at vendor/vext  third party, must be allowlisted

    fit.plane.lsq@1.10.0   registered by kfit   closure kfit, kgeo, knum
    geo.point.dist@1.2.0   registered by kgeo   closure kgeo, knum
    stat.mean.basic@2.0.0  registered by kstat  closure knum, kstat, vext

Written files (under fixtures/man/implclosure-ws-v1/):

    ws/**                  the toy sources (no Cargo.toml: the tool reads only metadata.json)
    metadata.json          `cargo metadata --format-version 1` output for the toy workspace,
                           with the fake workspace root /toy/ws
    inputs.json            algorithm table, registrations and allowlist (AC-1..AC-3)
    inputs-unlisted.json   the same with vext missing from the allowlist (AC-4)
    expected-table.json    canonical ImplDigestTable, no trailing newline (AC-1)

Digest rule (spec node KRN.MAN.MANI.IMPL, Design): for each algorithm, SHA-256 over the canonical
JSON array of one object per crate in the closure,
    {"features": [sorted], "files": [{"path": "src/..", "sha256": "sha256:<hex>"}, ..],
     "name": <name>, "version": <version>}
with crates sorted by (name, version) and files by the UTF-8 bytes of their path relative to the
crate directory; files are every regular file under src/ plus the build script.

Run:    python fixtures/man/make_implclosure_fixtures.py
Check:  python fixtures/man/make_implclosure_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "implclosure-ws-v1"
ROOT = "/toy/ws"

# ---------------------------------------------------------------------------------------------
# The toy sources. Byte strings: CRLF and binary content are part of the test.

FILES = {
    "crates/knum/src/lib.rs": b"pub mod ops;\n\npub fn one() -> u32 {\n    1\n}\n",
    "crates/knum/src/ops/mod.rs": b"pub mod add;\n",
    "crates/knum/src/ops/add.rs": b"pub fn add(a: u32, b: u32) -> u32 {\n    a + b\n}\n",
    "crates/knum/src/table.bin": b"\x00\x01\r\n\xff\xfe",
    "crates/knum/tests/num.rs": b"#[test]\nfn one() {\n    assert_eq!(knum::one(), 1);\n}\n",
    "crates/kgeo/build.rs": b"fn main() {}\n",
    "crates/kgeo/src/lib.rs": b"pub fn dist(a: u32, b: u32) -> u32 {\r\n    a.abs_diff(b)\r\n}\r\n",
    "crates/kgeo/benches/b.rs": b"fn main() {}\n",
    "crates/kfit/src/lib.rs": b"mod a;\n\npub fn fit() -> u32 {\n    a::b::two()\n}\n",
    "crates/kfit/src/a.rs": b"pub mod b;\n",
    "crates/kfit/src/a/b.rs": b"pub fn two() -> u32 {\n    2\n}\n",
    "crates/kfit/src/a-b.rs": b"// not a module: still under src/, so still hashed\n",
    "crates/kfit/examples/e.rs": b"fn main() {}\n",
    "crates/kstat/src/lib.rs": b"pub fn mean(a: u32, b: u32) -> u32 {\n    (a + b) / 2\n}\n",
    "crates/kstat/tests/mean.rs": b"#[test]\nfn mean() {}\n",
    "vendor/vext/src/lib.rs": b"pub const EXT: u32 = 7;\n",
}

# name, version, directory, features, targets (kind, path relative to the crate directory)
PACKAGES = [
    ("knum", "0.1.0", "crates/knum", ["std"], [("lib", "src/lib.rs"), ("test", "tests/num.rs")]),
    ("kgeo", "0.2.0", "crates/kgeo", [], [("lib", "src/lib.rs"), ("bench", "benches/b.rs"), ("custom-build", "build.rs")]),
    ("kfit", "0.3.0", "crates/kfit", ["default", "fast"], [("lib", "src/lib.rs"), ("example", "examples/e.rs")]),
    ("kstat", "0.1.0", "crates/kstat", [], [("lib", "src/lib.rs"), ("test", "tests/mean.rs")]),
    ("vext", "1.4.2", "vendor/vext", [], [("lib", "src/lib.rs")]),
]
MEMBERS = ["kfit", "kgeo", "knum", "kstat"]

# from, to, [(kind, target)] with kind None (normal), "build" or "dev"
EDGES = [
    ("kgeo", "knum", [(None, None)]),
    ("kfit", "kgeo", [(None, None)]),
    ("kfit", "knum", [(None, None), ("build", None)]),
    ("kstat", "knum", [(None, None)]),
    ("kstat", "vext", [("build", 'cfg(target_arch = "wasm32")')]),
    ("kstat", "kfit", [("dev", None)]),
]

ALGORITHMS = ["stat.mean.basic@2.0.0", "fit.plane.lsq@1.10.0", "geo.point.dist@1.2.0"]
REGISTRATIONS = [
    ("geo.point.dist@1.2.0", "kgeo"),
    ("fit.plane.lsq@1.10.0", "kfit"),
    ("stat.mean.basic@2.0.0", "kstat"),
]
ALLOWLIST = ["kfit", "kgeo", "knum", "kstat", "vext"]

# Hand-labelled from the edge list above (normal and build edges, any platform; no dev edges).
HAND_CLOSURES = {
    "fit.plane.lsq@1.10.0": ["kfit", "kgeo", "knum"],
    "geo.point.dist@1.2.0": ["kgeo", "knum"],
    "stat.mean.basic@2.0.0": ["knum", "kstat", "vext"],
}


def pkg_id(name, version, directory):
    return "path+file://%s/%s#%s@%s" % (ROOT, directory, name, version)


def metadata():
    by_name = {p[0]: p for p in PACKAGES}

    def idof(name):
        p = by_name[name]
        return pkg_id(p[0], p[1], p[2])

    packages = []
    for name, version, directory, features, targets in PACKAGES:
        deps = []
        for frm, to, kinds in EDGES:
            if frm != name:
                continue
            for kind, target in kinds:
                deps.append({
                    "name": to, "source": None, "req": "*", "kind": kind, "rename": None,
                    "optional": False, "uses_default_features": True, "features": [],
                    "target": target, "registry": None, "path": "%s/%s" % (ROOT, by_name[to][2]),
                })
        tgts = []
        for kind, path in targets:
            tgts.append({
                "kind": [kind], "crate_types": ["bin" if kind != "lib" else "lib"],
                "name": name if kind != "custom-build" else "build-script-build",
                "src_path": "%s/%s/%s" % (ROOT, directory, path),
                "edition": "2021", "doc": kind == "lib", "doctest": kind == "lib", "test": kind != "custom-build",
            })
        feats = {f: [] for f in features}
        packages.append({
            "name": name, "version": version, "id": pkg_id(name, version, directory),
            "license": None, "license_file": None, "description": None, "source": None,
            "dependencies": deps, "targets": tgts, "features": feats,
            "manifest_path": "%s/%s/Cargo.toml" % (ROOT, directory),
            "metadata": None, "publish": None, "authors": [], "categories": [], "keywords": [],
            "readme": None, "repository": None, "homepage": None, "documentation": None,
            "edition": "2021", "links": None, "default_run": None, "rust_version": None,
        })
    nodes = []
    for name, version, directory, features, _ in sorted(PACKAGES, key=lambda p: pkg_id(p[0], p[1], p[2])):
        out = [(to, kinds) for frm, to, kinds in EDGES if frm == name]
        nodes.append({
            "id": pkg_id(name, version, directory),
            "dependencies": sorted(idof(to) for to, _ in out),
            "deps": [
                {"name": to, "pkg": idof(to),
                 "dep_kinds": [{"kind": k, "target": t} for k, t in kinds]}
                for to, kinds in sorted(out, key=lambda e: idof(e[0]))
            ],
            "features": sorted(features),
        })
    members = [idof(m) for m in MEMBERS]
    doc = {
        "packages": packages, "workspace_members": members, "workspace_default_members": members,
        "resolve": {"nodes": nodes, "root": None}, "target_directory": ROOT + "/target",
        "version": 1, "workspace_root": ROOT, "metadata": None,
    }
    return (json.dumps(doc, separators=(",", ":")) + "\n").encode("utf-8")


def inputs(allowlist):
    doc = {
        "algorithms": ALGORITHMS,
        "registrations": [{"algorithm": a, "crate": c} for a, c in REGISTRATIONS],
        "allowlist": allowlist,
    }
    return (json.dumps(doc, indent=2) + "\n").encode("utf-8")


# ---------------------------------------------------------------------------------------------
# The oracle: reads metadata.json and the sources, never the Rust code.


def canon(value):
    def ascii_strings(v):
        if isinstance(v, str):
            assert v.isascii(), v
        elif isinstance(v, list):
            for x in v:
                ascii_strings(x)
        elif isinstance(v, dict):
            for k, x in v.items():
                ascii_strings(k)
                ascii_strings(x)

    ascii_strings(value)
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def alg_key(i):
    """KRN.MAN.CARD total order: module, feature, method, then the numeric version."""
    name, version = i.split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


class NotAllowlisted(Exception):
    pass


def oracle(meta_bytes, inputs_bytes, files):
    meta = json.loads(meta_bytes)
    inp = json.loads(inputs_bytes)
    root = meta["workspace_root"]
    pkgs = {p["id"]: p for p in meta["packages"]}
    edges = {}
    for node in meta["resolve"]["nodes"]:
        edges[node["id"]] = (
            [d["pkg"] for d in node["deps"] if any(k["kind"] in (None, "build") for k in d["dep_kinds"])],
            node["features"],
        )
    reg = {a: c for a, c in ((r["algorithm"], r["crate"]) for r in inp["registrations"])}
    allow = set(inp["allowlist"])

    def entry(pid):
        p = pkgs[pid]
        crate_dir = p["manifest_path"][len(root) + 1:-len("/Cargo.toml")]
        listed = []
        for rel, data in files.items():
            if rel.startswith(crate_dir + "/src/"):
                listed.append((rel[len(crate_dir) + 1:], data))
        for t in p["targets"]:
            if "custom-build" in t["kind"]:
                rel = t["src_path"][len(root) + 1:]
                listed.append((rel[len(crate_dir) + 1:], files[rel]))
        listed.sort(key=lambda f: f[0].encode("utf-8"))
        return {
            "features": sorted(edges[pid][1]),
            "files": [{"path": path, "sha256": sha(data)} for path, data in listed],
            "name": p["name"],
            "version": p["version"],
        }

    table = []
    closures = {}
    for alg in sorted(set(inp["algorithms"]), key=alg_key):
        start = [pid for pid, p in pkgs.items() if p["name"] == reg[alg]]
        assert len(start) == 1
        seen = set(start)
        queue = list(start)
        while queue:
            nxt = []
            for pid in queue:
                for dep in edges[pid][0]:
                    if dep not in seen:
                        seen.add(dep)
                        nxt.append(dep)
            queue = nxt
        crates = sorted(seen, key=lambda pid: (pkgs[pid]["name"].encode(), pkgs[pid]["version"].encode()))
        for pid in crates:
            if pkgs[pid]["name"] not in allow:
                raise NotAllowlisted(pkgs[pid]["name"])
        closures[alg] = [pkgs[pid]["name"] for pid in crates]
        table.append({"algorithm": alg, "impl_digest": sha(canon([entry(pid) for pid in crates]))})
    return table, closures


def build():
    out = {}
    for rel, data in FILES.items():
        out["ws/" + rel] = data
    meta = metadata()
    out["metadata.json"] = meta
    out["inputs.json"] = inputs(ALLOWLIST)
    out["inputs-unlisted.json"] = inputs([a for a in ALLOWLIST if a != "vext"])
    table, closures = oracle(meta, out["inputs.json"], FILES)
    assert closures == HAND_CLOSURES, closures
    try:
        oracle(meta, out["inputs-unlisted.json"], FILES)
        raise AssertionError("inputs-unlisted.json must fail")
    except NotAllowlisted as e:
        assert str(e) == "vext", e
    # Hand-labelled AC-2 expectations, checked against the oracle itself.
    mutated = dict(FILES)
    mutated["crates/kfit/src/a/b.rs"] = FILES["crates/kfit/src/a/b.rs"].replace(b"2", b"3")
    table2, _ = oracle(meta, out["inputs.json"], mutated)
    changed = [a["algorithm"] for a, b in zip(table, table2) if a != b]
    assert changed == ["fit.plane.lsq@1.10.0"], changed
    mutated = dict(FILES)
    mutated["crates/knum/tests/num.rs"] = b"\n"
    assert oracle(meta, out["inputs.json"], mutated)[0] == table
    out["expected-table.json"] = canon(table)
    return out


def main():
    check = "--check" in sys.argv[1:]
    out = build()
    on_disk = set()
    if OUT.exists():
        on_disk = {p.relative_to(OUT).as_posix() for p in OUT.rglob("*") if p.is_file()}
    stale = sorted(on_disk - set(out))
    bad = []
    for rel, data in sorted(out.items()):
        path = OUT / rel
        if check:
            if not path.is_file() or path.read_bytes() != data:
                bad.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        if bad or stale:
            for rel in bad:
                print("stale or missing: " + rel)
            for rel in stale:
                print("unexpected file: " + rel)
            sys.exit(1)
        print("implclosure-ws-v1: %d files up to date" % len(out))
    elif stale:
        for rel in stale:
            print("unexpected file (remove by hand): " + rel)
        sys.exit(1)


if __name__ == "__main__":
    main()
