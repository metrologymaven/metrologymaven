#!/usr/bin/env python3
"""Independent reference for AUTO.SURF.GEN (surface generator framework).

Reads the synthetic manifest fixtures/manifest/min-classes.json (written by
fixtures/manifest/make_fixtures.py) and writes expected-min-classes.json: the exact bytes
`cargo xtask gen-surfaces --test-emitter echo` must write, as a JSON object from the path
under --out to the file text. It is computed from the spec rules below, never from the Rust
code.

Stdlib only. `--check` regenerates in memory and fails if the committed file differs.

Pinned rules (mirrored by crates/fid-surface-gen, stated in the PR):
- header: three lines "generated, do not edit", "manifest sha256:<hex of the manifest
  file bytes>", "generator fid-surface-gen 1", each as "<prefix> <text>\\n" for a line
  comment; the echo emitter uses "#";
- echo writes echo/commands/<name>.v<major>.txt with "<name>@<major>\\n<title>\\n";
- echo/surface-index.json is RFC 8785 JSON without a trailing newline:
  {"_header": the three header lines joined by "; ", "commands": [{"command_id", "major"}]
  sorted by command ID bytes, "generator_version": "1", "manifest_digest", "surface": "echo"}.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
MANIFEST = ROOT / "fixtures" / "manifest" / "min-classes.json"
OUT = Path(__file__).resolve().parent / "expected-min-classes.json"

GENERATOR = "fid-surface-gen"
GENERATOR_VERSION = "1"


def canon(v):
    """RFC 8785 for the values used here: ASCII keys, small integers, strings."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def header_lines(digest):
    return ["generated, do not edit", f"manifest {digest}", f"generator {GENERATOR} {GENERATOR_VERSION}"]


def normalise(text):
    text = text.removeprefix("﻿").replace("\r\n", "\n").replace("\r", "\n")
    if text and not text.endswith("\n"):
        text += "\n"
    return text


def expected():
    raw = MANIFEST.read_bytes()
    digest = "sha256:" + hashlib.sha256(raw).hexdigest()
    manifest = json.loads(raw.decode("utf-8"))
    head = "".join(f"# {line}\n" for line in header_lines(digest))
    files = {}
    ids = {}
    for c in manifest["commands"]:
        cid = f"{c['id']}@{c['major']}"
        files[f"echo/commands/{c['id']}.v{c['major']}.txt"] = head + normalise(f"{cid}\n{c['title']}\n")
        ids[cid] = c["major"]
    index = {
        "_header": "; ".join(header_lines(digest)),
        "commands": [{"command_id": k, "major": ids[k]} for k in sorted(ids, key=lambda s: s.encode("utf-8"))],
        "generator_version": GENERATOR_VERSION,
        "manifest_digest": digest,
        "surface": "echo",
    }
    files["echo/surface-index.json"] = canon(index).decode("utf-8")
    return canon(files)


def main():
    data = expected()
    if "--check" in sys.argv[1:]:
        if OUT.read_bytes() != data:
            print(f"{OUT.relative_to(ROOT)} is stale; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print("AUTO.SURF.GEN fixtures up to date")
        return 0
    OUT.write_bytes(data)
    return 0


if __name__ == "__main__":
    sys.exit(main())
