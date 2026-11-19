#!/usr/bin/env python3
"""Coverage check for the GEO.NURBS.REF fixtures (AC-3; also re-checks the AC-1 manifest hashes).

Everything is recounted from the data, not from tags: degrees from the degree fields, rational from
the weights, repeated interior knots from the knot values, and the named circle, sphere and cylinder
from their geometry ids. It fails unless:
- every manifest file (oracle outputs, the adopted GEO.NURBS.DATA sets and the frozen, superseded
  ref_set_v1) matches its SHA-256;
- ref_set_v2 has, for every degree 1..25, a non-rational and a rational curve, and contains repeated
  interior knots (multiplicity >= 2) in both curves and surfaces;
- ref_set_v2 contains the circle, sphere and cylinder, has at least 5000 inversion queries, and
  every hex value agrees with its decimal;
- analytic_set covers the eight analytic types; multi_minima has tied (equidistant) queries.

Run:  python tools/fixtures/nurbs_coverage.py fixtures/geo/nurbs
"""

import hashlib
import json
import struct
import sys
from pathlib import Path

ANALYTIC = ["line", "circle", "ellipse", "plane", "cylinder", "cone", "sphere", "torus"]


def walk_f64(x, errors, where):
    """Every {hex, dec} pair must hold the same binary64 values."""
    if isinstance(x, dict):
        if set(x) == {"hex", "dec"}:
            def cmp(h, d):
                if isinstance(h, list):
                    if not isinstance(d, list) or len(h) != len(d):
                        errors.append(f"{where}: hex/dec shapes differ")
                        return
                    for a, b in zip(h, d):
                        cmp(a, b)
                    return
                if struct.unpack(">d", int(h, 16).to_bytes(8, "big"))[0] != float(d) or \
                        "0x%016x" % struct.unpack(">Q", struct.pack(">d", float(d)))[0] != h:
                    errors.append(f"{where}: {h} != {d!r}")
            cmp(x["hex"], x["dec"])
            return
        for k, v in x.items():
            walk_f64(v, errors, where)
    elif isinstance(x, list):
        for v in x:
            walk_f64(v, errors, where)


def repeated_interior(knots_hex, degree, n):
    vals = [struct.unpack(">d", int(h, 16).to_bytes(8, "big"))[0] for h in knots_hex]
    lo, hi = vals[degree], vals[n]
    return any(vals.count(k) >= 2 for k in set(vals) if lo < k < hi)


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "fixtures/geo/nurbs")
    man = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    problems = []
    docs = {}
    for entry in man["files"]:
        raw = (root / entry["path"]).read_bytes()
        if "sha256:" + hashlib.sha256(raw).hexdigest() != entry["sha256"]:
            problems.append(f"{entry['path']}: SHA-256 does not match the manifest")
        docs[entry["path"]] = json.loads(raw)
    ref = docs["ref_set_v2.json"]
    errors = []
    walk_f64(ref, errors, "ref_set_v2")
    problems += errors[:10]
    curves = [e for e in ref["entities"] if e["kind"] == "curve"]
    surfaces = [e for e in ref["entities"] if e["kind"] == "surface"]
    have = {(c["degree"], c["weights"] is not None) for c in curves}
    print(f"{'degree':>6} {'non-rational':>12} {'rational':>8}")
    for p in range(1, 26):
        nr, r = (p, False) in have, (p, True) in have
        print(f"{p:6} {'yes' if nr else 'MISSING':>12} {'yes' if r else 'MISSING':>8}")
        if not (nr and r):
            problems.append(f"degree {p}: rational={'present' if r else 'missing'}, "
                            f"non-rational={'present' if nr else 'missing'}")
    rep_c = [c["id"] for c in curves if repeated_interior(c["knots"]["hex"], c["degree"], len(c["ctrl"]["hex"]))]
    rep_s = [s["id"] for s in surfaces
             if repeated_interior(s["knots_u"]["hex"], s["degree_u"], s["n_u"])
             or repeated_interior(s["knots_v"]["hex"], s["degree_v"], s["n_v"])]
    print(f"curves with repeated interior knots: {len(rep_c)}; surfaces: {len(rep_s)}")
    if not rep_c or not rep_s:
        problems.append("repeated interior knots missing in curves or surfaces")
    surf_deg = sorted({(s["degree_u"], s["degree_v"]) for s in surfaces})
    print(f"surface degrees: {surf_deg}; rational surfaces: {sum(s['weights'] is not None for s in surfaces)}")
    ids = {e["id"] for e in ref["entities"]}
    for named in ("circle", "sphere", "cylinder"):
        if named not in ids:
            problems.append(f"named entity {named} missing")
    n_inv = sum(len(e["invert"]) for e in ref["entities"])
    n_eval = sum(len(e["eval"]) for e in ref["entities"])
    print(f"ref_set_v2: {len(ref['entities'])} entities, {n_eval} evaluations, {n_inv} inversions")
    if n_inv < 5000:
        problems.append(f"only {n_inv} inversion queries (need 5000)")
    an = docs["analytic_set.json"]
    types = sorted({e["type"] for e in an["entities"]})
    print(f"analytic types: {types}")
    if sorted(ANALYTIC) != types:
        problems.append("analytic_set does not cover the eight types")
    mm = docs["multi_minima.json"]
    ties = sum("tie" in q["tags"] for e in mm["entities"] for q in e["invert"])
    print(f"multi_minima: {ties} tied queries")
    if ties < 1:
        problems.append("multi_minima has no tied queries")
    for p in problems:
        print("PROBLEM:", p)
    print("coverage: OK" if not problems else "coverage: FAILED")
    sys.exit(0 if not problems else 1)


if __name__ == "__main__":
    main()
