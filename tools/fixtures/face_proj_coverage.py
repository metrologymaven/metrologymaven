#!/usr/bin/env python3
"""Coverage check for GEO.DIST.FACE.REF fixtures (AC-4, REQ-GEO.DIST.FACE.REF-003).

For every body file listed in the manifest it recounts, from the numbers (not only the
tags), the queries whose trim-ignoring answer is shorter than the true answer by more than
1e-9 m (the untrimmed minimum lies outside the trim) and the queries within 1e-6 m of an
edge, and fails unless both reach 50. It also checks the manifest SHA-256 of each file.

Run:  python tools/fixtures/face_proj_coverage.py fixtures/geo/dist/face/ref_v1
"""

import hashlib
import json
import sys
from pathlib import Path

NEED = 50


def main():
    root = Path(sys.argv[1] if len(sys.argv) > 1 else "fixtures/geo/dist/face/ref_v1")
    man = json.loads((root / "manifest.json").read_text(encoding="utf-8"))
    ok = True
    print(f"{'file':28} {'queries':>7} {'trimmed_away':>12} {'near_edge':>9} {'equidistant':>11} {'tie':>4}")
    for entry in man["files"]:
        raw = (root / entry["path"]).read_bytes()
        if "sha256:" + hashlib.sha256(raw).hexdigest() != entry["sha256"]:
            print(f"{entry['path']}: SHA-256 does not match the manifest")
            ok = False
        doc = json.loads(raw)
        qs = doc["queries"]
        trim = edge = 0
        for q in qs:
            is_trim = q["untrimmed"]["distance"]["dec"] < q["distance"]["dec"] - 1e-9
            ed = q["edge_distance"]
            is_edge = ed is not None and ed["dec"] <= 1e-6
            if is_trim != ("trimmed_away" in q["tags"]) or is_edge != ("near_edge" in q["tags"]):
                print(f"{entry['path']} {q['id']}: tags disagree with the numbers")
                ok = False
            trim += is_trim
            edge += is_edge
        eq = sum("equidistant" in q["tags"] for q in qs)
        tie = sum("tie" in q["tags"] for q in qs)
        flag = "" if trim >= NEED and edge >= NEED else "  <-- below 50"
        ok = ok and trim >= NEED and edge >= NEED
        print(f"{entry['path']:28} {len(qs):7} {trim:12} {edge:9} {eq:11} {tie:4}{flag}")
    print("coverage: OK" if ok else "coverage: FAILED")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
