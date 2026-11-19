"""Reference generator for the IO.REG.DETECT fixtures and their expected outcomes.

Independent of the Rust code. Writes, under fixtures/io/reg/detect/:

  registry.json                  the AC-1 test registry: five synthetic read descriptors
  ply_named.stl                  a hand-made ASCII PLY triangle named .stl (AC-2, AC-3)
  binary_stl_solid_header.stl    a one-facet binary STL whose header starts with "solid" (AC-4)
  random_64k.bin                 65,536 bytes from Python's random.Random(1) (AC-5)
  list_import_any.json           canonical `fid import list-formats --json` output (AC-1)
  expected.txt                   expected outcomes, derived below from the spec's rules

Every file is synthetic and hand-made here: no third-party samples, no QIF XSDs (D-038).
The expected outcomes come from `reference_detect`, a plain re-statement of
spec/io/reg/detect/NODE.md (REQ-002..005 and the pinned Design rules), not from the Rust code.

Run from the repository root:
  python fixtures/io/reg/detect/make_fixtures.py          (rewrite)
  python fixtures/io/reg/detect/make_fixtures.py --check  (fail if any file differs)
"""

import hashlib
import json
import random
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PREFIX = 64 * 1024

# ---- the test registry ------------------------------------------------------------------
SCHEMA = {"type": "object", "additionalProperties": False, "properties": {}}


def desc(fid, sigs, exts, origin="first_party"):
    return {
        "id": fid,
        "versions": {"min": "1", "max": "*"},
        "signatures": sigs,
        "extensions": exts,
        "options_schema": SCHEMA,
        "phase": "P0",
        "origin": origin,
        "direction": "read",
    }


REGISTRY = {
    "formats": [
        # Registration order is deliberately not ID order.
        desc("mesh.stl-ascii", [{"offset": 0, "priority": 10, "bytes": b"solid".hex()}], ["stl"]),
        desc("mesh.stl", [{"offset": 0, "priority": 20, "predicate": "stl_binary_size"}], ["stl"]),
        desc(
            "mesh.ply",
            [
                {"offset": 0, "priority": 10, "bytes": b"ply\n".hex()},
                {"offset": 0, "priority": 10, "bytes": b"ply\r\n".hex()},
            ],
            ["ply"],
        ),
        desc(
            "qif.xml",
            [{"offset": 0, "priority": 10, "contains": b"<QIFDocument".hex(), "window": 1024}],
            ["qif", "xml"],
        ),
        desc("cloud.e57", [{"offset": 0, "priority": 10, "bytes": b"ASTM-E57".hex()}], ["e57"]),
    ]
}

# ---- the files ----------------------------------------------------------------------------
PLY = (
    b"ply\n"
    b"format ascii 1.0\n"
    b"comment synthetic fixture for IO.REG.DETECT\n"
    b"element vertex 3\n"
    b"property float x\n"
    b"property float y\n"
    b"property float z\n"
    b"element face 1\n"
    b"property list uchar int vertex_indices\n"
    b"end_header\n"
    b"0 0 0\n"
    b"1 0 0\n"
    b"0 1 0\n"
    b"3 0 1 2\n"
)

header = b"solid synthetic binary STL for IO.REG.DETECT"
header = header + b" " * (80 - len(header))
facet = struct.pack("<12fH", 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0)
BIN_STL = header + struct.pack("<I", 1) + facet
assert len(BIN_STL) == 84 + 50

RANDOM = random.Random(1).randbytes(PREFIX)


# ---- reference detector (spec rules, restated) ---------------------------------------------
def sig_matches(sig, prefix, total_len):
    off = sig["offset"]
    if "bytes" in sig:
        b = bytes.fromhex(sig["bytes"])
        return prefix[off : off + len(b)] == b
    if "contains" in sig:
        return bytes.fromhex(sig["contains"]) in prefix[off : off + sig["window"]]
    assert sig["predicate"] == "stl_binary_size"
    if len(prefix) < 84:
        return False
    (n,) = struct.unpack("<I", prefix[80:84])
    return total_len == 84 + 50 * n


def extension(name):
    last = name.replace("\\", "/").rsplit("/", 1)[-1]
    if "." not in last:
        return None
    stem, ext = last.rsplit(".", 1)
    if not stem or not ext or len(ext) > 16 or not (ext.isascii() and ext.isalnum()):
        return None
    return ext.lower()


def reference_detect(registry, data, name, named=None):
    readers = [d for d in registry["formats"] if d["direction"] == "read"]
    ids = {d["id"] for d in readers}
    if named is not None:
        return ("ok", named) if named in ids else ("format_unknown", named)
    prefix = data[:PREFIX]
    hits = []
    for d in readers:
        ps = [s["priority"] for s in d["signatures"] if sig_matches(s, prefix, len(data))]
        if ps:
            hits.append((max(ps), d["id"]))
    if not hits:
        return ("format_unknown",)
    hits.sort(key=lambda h: (-h[0], h[1]))
    top = hits[0][0]
    tied = [h[1] for h in hits if h[0] == top]
    if len(tied) > 1:
        return ("format_ambiguous", top, tied)
    detected = tied[0]
    ext = extension(name) if name else None
    if ext is not None:
        by_ext = sorted(d["id"] for d in readers if ext in d["extensions"])
        if by_ext and detected not in by_ext:
            return ("format_mismatch", detected, ext, by_ext)
    return ("ok", detected)


def canonical(v):
    # RFC 8785 for this ASCII-only, integer-only content: sorted keys, no whitespace.
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def listing(registry):
    out = []
    for d in sorted(registry["formats"], key=lambda d: d["id"]):
        d = dict(d)
        d["extensions"] = sorted(d["extensions"])
        out.append(d)
    return out


def build():
    files = {}
    files["registry.json"] = (json.dumps(REGISTRY, indent=2) + "\n").encode("ascii")
    files["ply_named.stl"] = PLY
    files["binary_stl_solid_header.stl"] = BIN_STL
    files["random_64k.bin"] = RANDOM

    result = listing(REGISTRY)
    envelope = {"command_id": "import.list_formats@1", "envelope_version": 1, "outcome": "computed", "result": result}
    files["list_import_any.json"] = canonical(envelope)

    ac2 = reference_detect(REGISTRY, PLY, "ply_named.stl")
    ac3 = reference_detect(REGISTRY, PLY, "ply_named.stl", named="mesh.ply")
    ac4 = reference_detect(REGISTRY, BIN_STL, "binary_stl_solid_header.stl")
    ac5 = reference_detect(REGISTRY, RANDOM, "random_64k.bin")
    # Hand-labelled expectations from the acceptance criteria, checked against the reference.
    assert ac2 == ("format_mismatch", "mesh.ply", "stl", ["mesh.stl", "mesh.stl-ascii"]), ac2
    assert ac3 == ("ok", "mesh.ply"), ac3
    assert ac4 == ("ok", "mesh.stl"), ac4
    assert ac5 == ("format_unknown",), ac5
    # AC-4 really is a tie-break: both STL signatures match.
    assert BIN_STL.startswith(b"solid")

    expected = [
        ("list.count", len(result)),
        ("list.ids", ",".join(d["id"] for d in result)),
        ("list.result_sha256", "sha256:" + hashlib.sha256(canonical(result)).hexdigest()),
        ("ac2.code", ac2[0]),
        ("ac2.detected", ac2[1]),
        ("ac2.extension", ac2[2]),
        ("ac2.extension_formats", ",".join(ac2[3])),
        ("ac3.selected", ac3[1]),
        ("ac4.selected", ac4[1]),
        ("ac5.code", ac5[0]),
        ("ac5.max_read", PREFIX),
        ("random_64k.size", len(RANDOM)),
    ]
    files["expected.txt"] = (
        "# Generated by make_fixtures.py. key value\n" + "".join(f"{k} {v}\n" for k, v in expected)
    ).encode("ascii")
    return files


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, data in build().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"DIFFERS: {path}")
                bad += 1
        else:
            path.write_bytes(data)
    if check:
        print("io/reg/detect fixtures:", "differ" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
