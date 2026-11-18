#!/usr/bin/env python3
"""External-tool checks of a Result Package (TRUST.PKG.MANIFEST AC-2, AC-3).

Given a container (default: fixtures/pkg/golden/minimal.zip), this script:

- AC-2: lists it with Info-ZIP `unzip -Z1` (if on PATH) and with Python `zipfile`,
  and checks that both give exactly the manifest members plus statement.json,
  in that order, with manifest.json first;
- AC-3: validates manifest.json against schema/manifest-1.json (needs the
  `jsonschema` package; skipped with a note if it is missing) and hashes every
  member with `sha256sum` (if on PATH) and `hashlib`, comparing with the manifest.

It never runs any Rust code. Exit status 0 means every available check passed.
Run:  python fixtures/pkg/check_package.py [path/to/package.zip]
"""

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def main():
    pkg = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT / "fixtures/pkg/golden/minimal.zip"
    z = zipfile.ZipFile(pkg)
    manifest = json.loads(z.read("manifest.json"))
    member_paths = [m["path"] for m in manifest["members"]]
    expected = ["manifest.json"] + member_paths + ["statement.json"]

    names = z.namelist()
    assert names == expected, f"zipfile listing {names} != {expected}"
    print(f"zipfile: {len(names)} entries in manifest order")
    if shutil.which("unzip"):
        out = subprocess.run(["unzip", "-Z1", str(pkg)], check=True, capture_output=True).stdout
        listed = out.decode("utf-8").splitlines()
        assert listed == expected, f"unzip listing {listed} != {expected}"
        subprocess.run(["unzip", "-tq", str(pkg)], check=True)
        print("unzip: listing matches, archive tests OK")
    else:
        print("unzip: not on PATH, skipped")

    try:
        import jsonschema  # noqa: PLC0415
    except ImportError:
        print("schema: jsonschema not installed, skipped")
    else:
        schema = json.loads((ROOT / "schema/manifest-1.json").read_text(encoding="utf-8"))
        jsonschema.Draft202012Validator.check_schema(schema)
        jsonschema.Draft202012Validator(schema).validate(manifest)
        print("schema: manifest.json is valid against schema/manifest-1.json")

    with tempfile.TemporaryDirectory() as tmp:
        for m in manifest["members"]:
            data = z.read(m["path"])
            assert str(len(data)) == m["length"], m["path"]
            assert hashlib.sha256(data).hexdigest() == m["sha256"], m["path"]
            if shutil.which("sha256sum"):
                f = Path(tmp) / "member"
                f.write_bytes(data)
                out = subprocess.run(["sha256sum", str(f)], check=True, capture_output=True).stdout
                assert out.decode("ascii").split()[0].lstrip("\\") == m["sha256"], m["path"]
    tool = "sha256sum and hashlib" if shutil.which("sha256sum") else "hashlib (sha256sum not on PATH)"
    print(f"digests: {len(member_paths)} members match their manifest entries ({tool})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
