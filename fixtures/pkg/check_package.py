#!/usr/bin/env python3
"""External-tool checks of Result Packages (TRUST.PKG.MANIFEST AC-2, AC-3).

For each container, this script:

- AC-2: lists it with Info-ZIP `unzip -Z1` and with Python `zipfile`, and checks
  that both give exactly the manifest members plus statement.json, in that order,
  with manifest.json first; `unzip -t` must report no errors;
- AC-3: validates manifest.json against schema/manifest-1.json with `jsonschema`
  and hashes every member with `sha256sum` and `hashlib`, comparing with the manifest.

With no package arguments it checks the golden packages: the committed
`golden/minimal.zip`, plus the `mixed` contents and the forced-zip64 variants
rebuilt by the independent reference `make_fixtures.py`; each rebuilt container
must also match its pinned digest in `golden/*.sha256`.

`--strict` (used in CI) turns a missing `unzip`, `sha256sum` or `jsonschema` into
a failure instead of a skipped check. It never runs any Rust code.

Run:  python fixtures/pkg/check_package.py [--strict] [package.zip ...]
"""

import hashlib
import json
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


class Missing(Exception):
    pass


def need(tool, strict):
    if shutil.which(tool):
        return True
    if strict:
        raise Missing(f"{tool} is required with --strict but is not on PATH")
    print(f"{tool}: not on PATH, skipped")
    return False


def check(pkg, strict):
    print(f"== {pkg.name}")
    z = zipfile.ZipFile(pkg)
    manifest = json.loads(z.read("manifest.json"))
    member_paths = [m["path"] for m in manifest["members"]]
    expected = ["manifest.json"] + member_paths + ["statement.json"]

    names = z.namelist()
    assert names == expected, f"zipfile listing {names} != {expected}"
    assert z.testzip() is None
    print(f"zipfile: {len(names)} entries in manifest order")
    if need("unzip", strict):
        out = subprocess.run(["unzip", "-Z1", str(pkg)], check=True, capture_output=True).stdout
        listed = out.decode("utf-8").splitlines()
        assert listed == expected, f"unzip listing {listed} != {expected}"
        subprocess.run(["unzip", "-tq", str(pkg)], check=True, capture_output=True)
        print("unzip: listing matches, archive tests OK")

    try:
        import jsonschema  # noqa: PLC0415
    except ImportError:
        if strict:
            raise Missing("jsonschema is required with --strict but is not installed") from None
        print("schema: jsonschema not installed, skipped")
    else:
        schema = json.loads((ROOT / "schema/manifest-1.json").read_text(encoding="utf-8"))
        jsonschema.Draft202012Validator.check_schema(schema)
        jsonschema.Draft202012Validator(schema).validate(manifest)
        print("schema: manifest.json is valid against schema/manifest-1.json")

    use_sha256sum = need("sha256sum", strict)
    with tempfile.TemporaryDirectory() as tmp:
        for m in manifest["members"]:
            data = z.read(m["path"])
            assert str(len(data)) == m["length"], m["path"]
            assert hashlib.sha256(data).hexdigest() == m["sha256"], m["path"]
            if use_sha256sum:
                f = Path(tmp) / "member"
                f.write_bytes(data)
                out = subprocess.run(["sha256sum", str(f)], check=True, capture_output=True).stdout
                assert out.decode("ascii").split()[0].lstrip("\\") == m["sha256"], m["path"]
    print(f"digests: {len(member_paths)} members match their manifest entries")


def golden_packages(tmp):
    """The committed minimal.zip plus reference-built mixed and zip64 containers."""
    sys.path.insert(0, str(HERE))
    import make_fixtures as ref  # noqa: PLC0415

    pkgs = [HERE / "golden/minimal.zip"]
    for name, contents in (("minimal", ref.MINIMAL), ("mixed", ref.MIXED)):
        for force, suffix in ((False, ".zip"), (True, ".zip64.zip")):
            if name == "minimal" and not force:
                continue
            data = ref.build_package(contents, force_zip64=force)[2]
            pinned = (HERE / f"golden/{name}{suffix}.sha256").read_text(encoding="ascii").split()[0]
            assert hashlib.sha256(data).hexdigest() == pinned, f"{name}{suffix} differs from its pinned digest"
            path = Path(tmp) / f"{name}{suffix}"
            path.write_bytes(data)
            pkgs.append(path)
    return pkgs


def main():
    args = sys.argv[1:]
    strict = "--strict" in args
    paths = [Path(a) for a in args if a != "--strict"]
    with tempfile.TemporaryDirectory() as tmp:
        try:
            for pkg in paths or golden_packages(tmp):
                check(pkg, strict)
        except Missing as e:
            print(f"error: {e}")
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
