#!/usr/bin/env python3
"""Independent reference for the dmt-reviewers/v1 roster digest (GDT.RES.ROWFLOW.ROSTER, REQ-004).

Stdlib only. Normalises a roster the way the spec describes (entries keyed by reviewer ID,
principals and dialects sorted, valid_until omitted when absent), writes RFC 8785 canonical
JSON (for these ASCII-only, string-only documents this is json.dumps with sorted keys and no
whitespace) and prints `sha256:<hex>`.

    python make_digest.py           # rewrite roster_ok.digest
    python make_digest.py --check   # verify roster_ok.digest and that the reordered file agrees

This does not validate rosters; the Rust loader's error paths are hand-labelled in
roster_ok_truth.json.
"""

import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
FIELDS = ("basis", "display_name", "valid_from", "valid_until")


def normalised(doc):
    entries = {}
    for e in doc["entries"]:
        o = {k: e[k] for k in FIELDS if k in e}
        o["principals"] = sorted(e["principals"])
        o["dialects"] = sorted(e["dialects"])
        entries[e["id"]] = o
    return {"entries": entries, "version": doc["version"]}


def digest(path):
    doc = json.loads(path.read_text(encoding="utf-8"))
    canon = json.dumps(normalised(doc), sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert canon.isascii(), "reference only covers ASCII rosters"
    return "sha256:" + hashlib.sha256(canon.encode("utf-8")).hexdigest()


def main():
    ok = digest(HERE / "roster_ok.json")
    reordered = digest(HERE / "roster_ok_reordered.json")
    if ok != reordered:
        sys.exit(f"reordered roster digest differs: {ok} != {reordered}")
    target = HERE / "roster_ok.digest"
    if "--check" in sys.argv[1:]:
        have = target.read_bytes().decode("ascii")
        if have != ok:
            sys.exit(f"roster_ok.digest is stale: {have} != {ok}")
        print("ok", ok)
    else:
        target.write_bytes(ok.encode("ascii"))
        print(ok)


if __name__ == "__main__":
    main()
