#!/usr/bin/env python3
"""Generate the UNC.PROP.LPU.TAN fixtures (CC-BY-4.0 project data, invented values).

Independent reference for `fid_kunc::prop::lpu::tan` (D-038): every expected value here comes
from the Python standard library (json, struct, hashlib) and from the rules written in
spec/unc/prop/lpu/tan/NODE.md, never from the Rust code.

Files written next to this script:

- affine_model.json        uncertainty-model/v1 document with one operator node pinned to
                           test.affine.map@1.0.0, params {"a": 3x3 matrix, "b": offset} (AC-1)
- affine_model_v110.json   the same model referencing test.affine.map@1.1.0 (AC-2)
- manifest_min.json        canonical kernel manifest (KRN.MAN.MANI.SCHEMA) listing 5 operators
                           (AC-4)
- coverage_expected.json   the tangent-coverage@1 report expected for manifest_min.json with
                           providers for 3 of its operators plus 1 orphan provider (AC-4)
- expected.txt             hand-checkable expectations: the affine Jacobian bit patterns (AC-1),
                           the AC-2 error and the AC-4 missing/orphan lists

Run:    python fixtures/unc/lpu/tan/make_fixtures.py
Check:  python fixtures/unc/lpu/tan/make_fixtures.py --check
"""

import hashlib
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).parent


def hex64(x):
    """The kernel wire form of an f64: its IEEE-754 bit pattern as 16 lowercase hex digits."""
    assert x == x and x not in (float("inf"), float("-inf"))
    assert not (x == 0.0 and struct.pack(">d", x)[0] & 0x80), "negative zero has no spelling"
    return struct.pack(">d", x).hex()


def h(label):
    """A stand-in digest: SHA-256 of a label, in the `sha256:<hex>` wire form."""
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def canon(value):
    """RFC 8785 for the ASCII-strings-only JSON used here: sorted keys, no whitespace."""
    text = json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert text.isascii()
    return text.encode("ascii")


# ---------------------------------------------------------------------------- AC-1, AC-2

# The affine operator y = A x + b. Invented values chosen so that no entry is a simple power of
# two (a wrong row/column order or a transposition cannot pass by accident).
A = [
    [1.25, -0.375, 2.0625],
    [0.0, 3.5, -1.125],
    [-0.8125, 0.4375, 1.75],
]
B = [0.001, -0.002, 0.0005]

INPUTS = [("x0", 0.012), ("x1", -0.034), ("x2", 0.056)]


def affine_model(version):
    doc = {
        "schema": "uncertainty-model/v1",
        "name": "affine-tan",
        "semver": "1.0.0",
        "inputs": [
            {
                "id": iid,
                "quantity_kind": "length",
                "unit": "si:m",
                "distribution": {"kind": "normal", "params": {"mu": hex64(mu), "sigma": hex64(1e-6)}},
                "source": {"kind": "declared", "digest": h("declared " + iid), "author": "metrology-maven fixtures"},
            }
            for iid, mu in INPUTS
        ],
        "operators": [
            {
                "id": "aff",
                "algorithm_id": "test.affine.map",
                "algorithm_version": version,
                "params": {"a": [[hex64(v) for v in row] for row in A], "b": [hex64(v) for v in B]},
                # One scalar per input port, in argument order: the Jacobian has one column per
                # port (arity 1 each) and one row per output port.
                "ports_in": [
                    {"name": "x0", "quantity_kind": "length", "unit": "si:m", "arity": "1"},
                    {"name": "x1", "quantity_kind": "length", "unit": "si:m", "arity": "1"},
                    {"name": "x2", "quantity_kind": "length", "unit": "si:m", "arity": "1"},
                ],
                "ports_out": [
                    {"name": "y0", "quantity_kind": "length", "unit": "si:m"},
                    {"name": "y1", "quantity_kind": "length", "unit": "si:m"},
                    {"name": "y2", "quantity_kind": "length", "unit": "si:m"},
                ],
            }
        ],
        "outputs": [{"id": "y%d" % i, "quantity_kind": "length", "unit": "si:m"} for i in range(3)],
        "edges": [{"from": {"node": "x%d" % i, "port": "value"}, "to": {"node": "aff", "port": "x%d" % i}} for i in range(3)]
        + [{"from": {"node": "aff", "port": "y%d" % i}, "to": {"node": "y%d" % i, "port": "value"}} for i in range(3)],
        "correlations": [],
    }
    return (json.dumps(doc, indent=2) + "\n").encode("ascii")


# ---------------------------------------------------------------------------------- AC-4


def alg_key(alg_id):
    """KRN.MAN.CARD total order: module, feature, method, then the numeric version."""
    name, version = alg_id.split("@")
    return (tuple(name.split(".")), tuple(int(x) for x in version.split(".")))


MANIFEST_IDS = [
    "test.affine.map@1.0.0",
    "test.poly.map@1.0.0",
    "fit.line.ls@1.0.0",
    "test.scale.map@2.0.0",
    "test.scale.map@10.0.0",  # after @2.0.0: versions compare as numbers, not as text
]

# Providers registered by the AC-4 test: three manifest operators plus one orphan whose
# version differs from the listed test.affine.map@1.0.0 (exact match only, REQ-001).
PROVIDER_IDS = [
    "test.affine.map@1.0.0",
    "test.poly.map@1.0.0",
    "fit.line.ls@1.0.0",
    "test.affine.map@1.1.0",
]


def manifest():
    core = {
        "schema_version": "1",
        "name": "fid-kernel",
        "semver": "0.1.0",
        "numeric_profile": "knp-1",
        "source_tree_digest": h("source tree"),
        "lockfile_digest": h("Cargo.lock"),
        "toolchain_digest": h("toolchain and flags"),
        "replay_runtime": {"version": "0.1.0", "config_digest": h("replay config")},
        "algorithms": [
            {"id": i, "card_digest": h("card " + i), "impl_digest": h("impl " + i)}
            for i in sorted(MANIFEST_IDS, key=alg_key)
        ],
        "facade_selftest_digest": h("facade self-test"),
        "solver_limits_digest": h("solver limits"),
        "reference_suite_digest": h("reference suite"),
        "reference_results_digest": h("reference results"),
        "native_gate": "measured-only",
    }
    whole = {
        "artifacts": [{"role": "wasm", "target": "wasm32-unknown-unknown", "sha256": h("wasm artifact")}],
        "core": core,
        "core_digest": "sha256:" + hashlib.sha256(canon(core)).hexdigest(),
    }
    return canon(whole)


def coverage(manifest_bytes):
    listed = set(MANIFEST_IDS)
    have = set(PROVIDER_IDS)
    report = {
        "manifest": "sha256:" + hashlib.sha256(manifest_bytes).hexdigest(),
        "missing": sorted(listed - have, key=alg_key),
        "orphan": sorted(have - listed, key=alg_key),
        "version": "tangent-coverage@1",
    }
    return report


def build():
    out = {}
    out["affine_model.json"] = affine_model("1.0.0")
    out["affine_model_v110.json"] = affine_model("1.1.0")
    m = manifest()
    out["manifest_min.json"] = m
    rep = coverage(m)
    out["coverage_expected.json"] = canon(rep)
    lines = ["# UNC.PROP.LPU.TAN expectations, written by make_fixtures.py (do not edit)."]
    lines.append("# jacobian <row> <col> <f64 bits>: the affine matrix A of affine_model.json (AC-1)")
    for i, row in enumerate(A):
        for j, v in enumerate(row):
            lines.append("jacobian %d %d %s" % (i, j, hex64(v)))
    lines.append("no_derivative test.affine.map@1.1.0")
    for x in rep["missing"]:
        lines.append("missing " + x)
    for x in rep["orphan"]:
        lines.append("orphan " + x)
    out["expected.txt"] = ("\n".join(lines) + "\n").encode("ascii")
    return out


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in build().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
