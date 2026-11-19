#!/usr/bin/env python3
"""Reference generator for the ALN.MODEL.ELEM fixture chain3 (CC-BY-4.0 project data).

Writes chain3/project.txt (the project the Rust tests build), chain3/expected.txt (IDs and
digests), chain3/get-a2.json and chain3/list.json (expected command output bytes). Every
expected value is computed here from the rules pinned in spec/aln/model/elem/NODE.md
(Design), not by running the Rust code:

- seed 1 drives a SplitMix64 stream; each element ID is a UUIDv7 (RFC 9562 section 5.7)
  from unix_ms = BASE_MS + k and the top 74 bits of two consecutive outputs;
- the stand-in mesh and the planes have content digests = SHA-256 of their canonical JSON;
- the stub solver (aln.stub.fixed) returns R = [[0,-1,0],[1,0,0],[0,0,1]] and
  t = (tx, ty, tz) in SI metres; version 1.0.0 returns a rigid transform, version 1.1.0 a
  similarity with s = 1.5; its residual report has a fixed ID and the digest SHA-256 of the
  input digests joined by newlines;
- SI values are the authored decimals converted with one correctly rounded division
  (Fraction -> float rounds to nearest, ties to even);
- floats are written as "0x" + 16 lowercase hex digits of their IEEE 754 bits;
- canonical JSON is RFC 8785; every value here is an ASCII string, null, array or object,
  so json.dumps with sorted keys and no whitespace is exactly RFC 8785.

Stdlib only. Run:  python fixtures/aln/model/make_fixtures.py [--check]
"""

import hashlib
import json
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "chain3"
MASK = (1 << 64) - 1
SEED = 1
BASE_MS = 1_763_510_400_000  # 2025-11-19T00:00:00Z
REPORT_ID = "0193a000-0000-7000-8000-0000000000aa"
UNITS = {"m": Fraction(1), "mm": Fraction(1, 1000), "um": Fraction(1, 10**6), "in": Fraction(127, 5000)}


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
    return state, z ^ (z >> 31)


def uuid_text(n):
    h = f"{n:032x}"
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def uuid_v7(ms, r):
    rand_a = r >> 62
    rand_b = r & ((1 << 62) - 1)
    return uuid_text((ms << 80) | (0x7 << 76) | (rand_a << 64) | (0b10 << 62) | rand_b)


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def bits(x):
    return "0x" + format(struct.unpack(">Q", struct.pack(">d", x))[0], "016x")


def si(text, unit):
    return float(Fraction(text) * UNITS[unit])


ROT = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]

# name, algorithm, parent, inputs, principal, kind, params (name, text, unit)
ALIGNMENTS = [
    ("A0", "aln.stub.fixed@1.0.0", None, ["P0"], "qa-1", "human",
     [("tx", "10", "mm"), ("ty", "-2.5", "mm"), ("tz", "0.125", "mm")]),
    ("A1", "aln.stub.fixed@1.1.0", "A0", ["P1"], "agent-1", "agent",
     [("tx", "1", "in"), ("ty", "0.5", "in"), ("tz", "-0.25", "in")]),
    ("A2", "aln.stub.fixed@1.0.0", "A1", ["P2"], "qa-1", "human",
     [("tx", "250", "um"), ("ty", "0", "um"), ("tz", "-125", "um")]),
]


def plane(origin, normal):
    return {"normal": [bits(x) for x in normal], "origin": [bits(x) for x in origin], "type": "plane"}


def transform(alg, t):
    v = {"kind": "rigid", "r": [[bits(x) for x in row] for row in ROT], "t": [bits(x) for x in t]}
    if alg.endswith("@1.1.0"):
        v["kind"] = "similarity"
        v["s"] = bits(1.5)
    return v


def build():
    state = SEED
    ids, obs = {}, {}
    names = ["M", "P0", "P1", "P2", "A0", "A1", "A2"]
    for k, name in enumerate(names):
        state, x1 = splitmix64(state)
        state, x2 = splitmix64(state)
        r = ((x1 << 64) | x2) >> 54
        ms = BASE_MS + k
        ids[name] = uuid_v7(ms, r)
        obs[name] = (ms, r)

    digests = {
        "M": sha(canon({"recipe": "block 10000 triangles seed 1 (stand-in)", "type": "mesh"})),
        "P0": sha(canon(plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]))),
        "P1": sha(canon(plane([0.0, 0.0, 0.05], [1.0, 0.0, 0.0]))),
        "P2": sha(canon(plane([0.0, 0.05, 0.0], [0.0, 1.0, 0.0]))),
    }
    shifted_p1 = sha(canon(plane([0.0, 0.0, 0.051], [1.0, 0.0, 0.0])))

    def solve(world, a):
        name, alg, parent, inputs, principal, kind, params = a
        in_refs = [{"digest": world[i], "id": ids[i]} for i in inputs]
        t = [si(text, unit) for (_, text, unit) in params]
        report = {"digest": sha("\n".join(r["digest"] for r in in_refs)), "id": REPORT_ID}
        parent_ref = None if parent is None else {"digest": world[parent], "id": ids[parent]}
        content = {
            "algorithm": alg,
            "inputs": [r["digest"] for r in in_refs],
            "params": {p: bits(si(text, unit)) for (p, text, unit) in params},
            "parent": parent_ref,
            "report": report["digest"],
            "transform": transform(alg, t),
        }
        digest = sha(canon(content))
        element = {
            "algorithm": alg,
            "digest": digest,
            "failure": None,
            "id": ids[name],
            "inputs": in_refs,
            "params": {p: {"si": bits(si(text, unit)), "text": text, "unit": unit} for (p, text, unit) in params},
            "parent": parent_ref,
            "provenance": {"kind": kind, "principal": principal, "state": "human" if kind == "human" else "proposed"},
            "report": report,
            "status": "ok",
            "transform": transform(alg, t),
        }
        return digest, element

    world = dict(digests)
    elements = {}
    for a in ALIGNMENTS:
        world[a[0]], elements[a[0]] = solve(world, a)
    before = {n: world[n] for n in ("A0", "A1", "A2")}

    # AC-2: P1 replaced by a copy shifted 1 mm along z; A1 then A2 recompute.
    world2 = dict(world)
    world2["P1"] = shifted_p1
    for a in ALIGNMENTS[1:]:
        world2[a[0]], _ = solve(world2, a)

    project = ["# chain3: generated by make_fixtures.py (seed 1). Do not edit by hand."]
    project.append("# element <name> <id> <content digest>")
    for n in ("M", "P0", "P1", "P2"):
        project.append(f"element {n} {ids[n]} {digests[n]}")
    project.append(f"shifted P1 {shifted_p1}")
    project.append("# edge <from> <to>: the planes were fitted to the mesh")
    for n in ("P0", "P1", "P2"):
        project.append(f"edge M {n}")
    project.append("# alignment <name> <algorithm> <parent|-> <inputs,> <principal> <kind> <ms> <random74 hex> <param=text:unit>...")
    for name, alg, parent, inputs, principal, kind, params in ALIGNMENTS:
        ms, r = obs[name]
        ps = " ".join(f"{p}={text}:{unit}" for (p, text, unit) in params)
        project.append(
            f"alignment {name} {alg} {parent or '-'} {','.join(inputs)} {principal} {kind} {ms} {r:x} {ps}"
        )
    project.append(f"report_id {REPORT_ID}")

    expected = ["# chain3 expected values: generated by make_fixtures.py. Do not edit by hand."]
    for n in ("A0", "A1", "A2"):
        expected.append(f"id {n} {ids[n]}")
    for n in ("A0", "A1", "A2"):
        expected.append(f"digest {n} {before[n]}")
    for n in ("A1", "A2"):
        expected.append(f"digest_after_shift {n} {world2[n]}")

    summaries = [
        {
            "algorithm": elements[n]["algorithm"],
            "digest": elements[n]["digest"],
            "id": ids[n],
            "parent": None if elements[n]["parent"] is None else elements[n]["parent"]["id"],
            "status": "ok",
        }
        for n in sorted(("A0", "A1", "A2"), key=lambda n: ids[n])
    ]
    return {
        "project.txt": "\n".join(project) + "\n",
        "expected.txt": "\n".join(expected) + "\n",
        "get-a2.json": canon(elements["A2"]),
        "list.json": canon(summaries),
    }


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    bad = 0
    for name, text in files.items():
        path = OUT / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode("utf-8"):
                print(f"STALE {path.relative_to(HERE)}")
                bad += 1
        else:
            OUT.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode("utf-8"))
    print("aln/model fixtures:", "stale" if bad else ("ok" if check else "written"))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
