#!/usr/bin/env python3
"""Generate the KRN.MAN.MANI.SCHEMA fixtures (CC-BY-4.0 project data).

Independent reference for the Rust `fid_kci::manifest` module: canonical JSON comes from
Python's json module (sorted keys, no whitespace; every value is an ASCII string, where that
equals RFC 8785) and digests from hashlib, never from the Rust code.

- fixtures/man/manifest-v1.json          canonical manifest, no trailing newline (AC-1)
- fixtures/man/manifest-v1-digests.txt   its core and manifest digests (AC-1)
- fixtures/man/manifest/*.json           copies with one change each (AC-2); their expected
                                         outcomes are hand-labelled in manifest/labels.txt

Run:    python fixtures/man/make_manifest_fixtures.py
Check:  python fixtures/man/make_manifest_fixtures.py --check
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
OUT = HERE / "manifest"


def h(label):
    """A stand-in digest: SHA-256 of a label, in the `sha256:<hex>` wire form."""
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


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


def alg_key(entry):
    """KRN.MAN.CARD total order: module, feature, method, then the numeric version."""
    name, version = entry["id"].split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


def alg(i):
    return {"id": i, "card_digest": h("card " + i), "impl_digest": h("impl " + i)}


IDS = [
    "unc.prop.gum@1.2.3",
    "fit.cylinder.ls@10.0.0",  # after @2.0.0: versions compare as numbers, not text
    "fit.cylinder.ls@2.0.0",
    "fit.cylinder.ls@1.0.0",
    "fit.plane.ls2@1.0.0",  # after fit.plane.ls: method "ls" < "ls2", although "ls@" > "ls2" as text
    "fit.plane.ls@1.0.0",
    "gdt.flatness.mz@0.1.0",
]

CORE = {
    "schema_version": "1",
    "name": "fid-kernel",
    "semver": "0.1.0",
    "numeric_profile": "knp-1",
    "source_tree_digest": h("source tree"),
    "lockfile_digest": h("Cargo.lock"),
    "toolchain_digest": h("toolchain and flags"),
    "replay_runtime": {"version": "0.1.0-alpha.1", "config_digest": h("replay config")},
    "algorithms": sorted([alg(i) for i in IDS], key=alg_key),
    "facade_selftest_digest": h("facade self-test"),
    "solver_limits_digest": h("solver limits"),
    "reference_suite_digest": h("reference suite"),
    "reference_results_digest": h("reference results"),
    "native_gate": "measured-only",
}
ARTIFACTS = [
    {"role": "native", "target": "aarch64-apple-darwin", "sha256": h("native aarch64-apple-darwin")},
    {"role": "native", "target": "x86_64-unknown-linux-gnu", "sha256": h("native x86_64-unknown-linux-gnu")},
    {"role": "wasm", "target": "wasm32-unknown-unknown", "sha256": h("wasm wasm32-unknown-unknown")},
]


def core_digest(core):
    return "sha256:" + hashlib.sha256(canon(core)).hexdigest()


def manifest(core=None, artifacts=None, digest=None):
    core = copy.deepcopy(CORE if core is None else core)
    return {
        "artifacts": copy.deepcopy(ARTIFACTS if artifacts is None else artifacts),
        "core": core,
        "core_digest": core_digest(core) if digest is None else digest,
    }


BASE = manifest()
BASE_BYTES = canon(BASE)
assert BASE["core"]["algorithms"] == sorted(BASE["core"]["algorithms"], key=alg_key)
assert [a["id"] for a in BASE["core"]["algorithms"]][0:3] == [
    "fit.cylinder.ls@1.0.0", "fit.cylinder.ls@2.0.0", "fit.cylinder.ls@10.0.0"]


def with_core(change):
    """A manifest whose core is changed by `change`, with a matching core_digest."""
    core = copy.deepcopy(CORE)
    change(core)
    return canon(manifest(core))


def set_in(path, value):
    def change(core):
        target = core
        for p in path[:-1]:
            target = target[p]
        target[path[-1]] = value
    return change


def swapped(i, j):
    def change(core):
        a = core["algorithms"]
        a[i], a[j] = a[j], a[i]
    return change


def text_replace(old, new):
    assert BASE_BYTES.count(old) == 1, old
    return BASE_BYTES.replace(old, new)


def order_by_text(core):
    core["algorithms"].sort(key=lambda e: e["id"])


def drop_core(key):
    def change(core):
        del core[key]
    return change


def top(change):
    m = copy.deepcopy(BASE)
    change(m)
    return canon(m)


def pretty():
    """Same manifest with whitespace and reversed member order everywhere."""
    def rev(v):
        if isinstance(v, dict):
            return {k: rev(v[k]) for k in reversed(sorted(v))}
        if isinstance(v, list):
            return [rev(x) for x in v]
        return v
    return (json.dumps(rev(BASE), indent=2) + "\n").encode("ascii")


lock = CORE["lockfile_digest"]
CASES = {
    "ok-whitespace.json": pretty(),
    "ok-escaped.json": text_replace(b'"name":"fid-kernel"', b'"name":"fid\\u002dkernel"'),
    "ok-u32-max.json": with_core(lambda c: c["algorithms"].append(alg("zzz.a.b@4294967295.0.0"))),
    "unknown-field-top.json": top(lambda m: m.__setitem__("extra", "x")),
    "unknown-field-core.json": with_core(set_in(["comment"], "x")),
    "unknown-field-algorithm.json": with_core(set_in(["algorithms", 0, "note"], "x")),
    "duplicate-key-core.json": text_replace(b'"name":"fid-kernel"', b'"name":"fid-kernel","name":"fid-kernel"'),
    "duplicate-key-escaped.json": text_replace(b'"name":"fid-kernel"', b'"name":"fid-kernel","n\\u0061me":"fid-kernel"'),
    "duplicate-key-artifact.json": text_replace(b'"role":"wasm"', b'"role":"wasm","role":"wasm"'),
    "unsorted-algorithms.json": with_core(swapped(0, 1)),
    "unsorted-algorithms-numeric.json": with_core(order_by_text),
    "unsorted-algorithms-method.json": with_core(swapped(3, 4)),
    "duplicate-algorithm.json": with_core(lambda c: c["algorithms"].insert(1, copy.deepcopy(c["algorithms"][0]))),
    "unsorted-artifacts.json": canon(manifest(artifacts=list(reversed(ARTIFACTS)))),
    "core-digest-mismatch.json": canon(manifest(digest=core_digest(CORE)[:-1] + ("0" if core_digest(CORE)[-1] != "0" else "1"))),
    "core-edited.json": canon(manifest(core=dict(CORE, semver="0.2.0"), digest=core_digest(CORE))),
    "bad-hex-upper.json": with_core(set_in(["lockfile_digest"], "sha256:" + lock[7:].upper())),
    "bad-hex-short.json": with_core(set_in(["lockfile_digest"], lock[:-1])),
    "bad-hex-long.json": with_core(set_in(["lockfile_digest"], lock + "0")),
    "bad-hex-bare.json": with_core(set_in(["lockfile_digest"], lock[7:])),
    "bad-hex-newline.json": with_core(set_in(["lockfile_digest"], lock + "\n")),
    "bad-hex-algorithm.json": with_core(set_in(["algorithms", 2, "impl_digest"], "sha512:" + lock[7:])),
    "bad-hex-artifact.json": canon(manifest(artifacts=[dict(ARTIFACTS[0], sha256="sha256:" + "g" * 64)] + ARTIFACTS[1:])),
    "bad-native-gate.json": with_core(set_in(["native_gate"], "on")),
    "bad-schema-version-number.json": with_core(set_in(["schema_version"], 1)),
    "bad-schema-version.json": with_core(set_in(["schema_version"], "2")),
    "bad-null.json": with_core(set_in(["name"], None)),
    "bad-type-algorithms.json": with_core(set_in(["algorithms"], {})),
    "bad-semver.json": with_core(set_in(["semver"], "0.01.0")),
    "bad-semver-build.json": with_core(set_in(["semver"], "0.1.0+build.1")),
    "bad-runtime-version.json": with_core(set_in(["replay_runtime", "version"], "0.1")),
    "bad-algorithm-id.json": with_core(set_in(["algorithms", 0, "id"], "Fit.cylinder.ls@1.0.0")),
    "bad-algorithm-id-u32.json": with_core(set_in(["algorithms", 6, "id"], "unc.prop.gum@4294967296.0.0")),
    "bad-name-newline.json": with_core(set_in(["name"], "fid-kernel\n")),
    "bad-name-upper.json": with_core(set_in(["name"], "Fid-kernel")),
    "bad-name-long.json": with_core(set_in(["name"], "f" * 65)),
    "bad-target.json": canon(manifest(artifacts=ARTIFACTS[:2] + [dict(ARTIFACTS[2], target="-wasm32")])),
    "missing-field.json": with_core(drop_core("facade_selftest_digest")),
    "missing-core-digest.json": top(lambda m: m.__delitem__("core_digest")),
    "syntax-trailing-comma.json": BASE_BYTES[:-1] + b",}",
    "syntax-trailing-data.json": BASE_BYTES + b"x",
    "syntax-truncated.json": BASE_BYTES[:-1],
    "syntax-bom.json": b"\xef\xbb\xbf" + BASE_BYTES,
    "syntax-raw-control.json": text_replace(b'"name":"fid-kernel"', b'"name":"fid-\tkernel"'),
}
assert BASE_BYTES not in CASES.values()

DIGESTS = (
    "# KRN.MAN.MANI.SCHEMA AC-1: digests of manifest-v1.json (hashlib). Format: <name><TAB><digest>.\n"
    f"core_digest\t{core_digest(CORE)}\n"
    f"manifest_digest\tsha256:{hashlib.sha256(BASE_BYTES).hexdigest()}\n"
).encode("ascii")


def outputs():
    yield HERE / "manifest-v1.json", BASE_BYTES
    yield HERE / "manifest-v1-digests.txt", DIGESTS
    for name, data in sorted(CASES.items()):
        yield OUT / name, data


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    expected = set()
    for path, data in outputs():
        expected.add(path.name)
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(str(path))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    extra = sorted(p.name for p in OUT.glob("*.json") if p.name not in expected)
    if extra:
        stale += [f"unexpected {OUT / x}" for x in extra]
    if stale:
        print("stale fixtures:\n  " + "\n  ".join(stale))
        sys.exit(1)
    print(f"{'checked' if check else 'wrote'} {len(expected)} files")


if __name__ == "__main__":
    main()
