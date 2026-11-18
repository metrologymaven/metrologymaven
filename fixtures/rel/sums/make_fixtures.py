#!/usr/bin/env python3
"""Independent reference for ECO.REL.SIGN.SUMS fixtures (CC-BY-4.0 project data).

Writes small dummy release artefacts and `release-manifest.json` files into
fixtures/rel/sums/{ok,extra-file,missing-file,bad-name}/ and the expected outputs of
`cargo xtask rel sums` for the `ok` case into fixtures/rel/sums/expected/:

- expected/SHA256SUMS: `<64 lowercase hex>  <name>` per artefact, sorted by name in byte
  order, LF line endings (hashlib SHA-256, not the Rust code);
- expected/sums-statement.json: {release, artefacts: [{name, sha256, size}]} as RFC 8785
  canonical JSON (sorted keys, no whitespace, no trailing newline). Sizes are decimal
  strings, the fid-canon rule for 64-bit integers; sha256 is the same 64 hex as SHA256SUMS.

The manifests also list `SHA256SUMS`, its DSSE envelope and an SPDX SBOM, which
REQ-ECO.REL.SIGN.SUMS-005 keeps out of SHA256SUMS. Only the fields the tool reads
(`version`, `artefacts[].name`, `artefacts[].kind`) plus a few realistic extras are written.

Run:  python fixtures/rel/sums/make_fixtures.py
"""

import hashlib
import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent
VERSION = "1.4.0"


def dummy(name, n):
    """Deterministic dummy bytes, NUL included so git keeps them binary."""
    seed = name.encode()
    out = bytearray(b"\x00dummy artefact " + seed + b"\n")
    i = 0
    while len(out) < n:
        out += hashlib.sha256(seed + i.to_bytes(4, "big")).digest()
        i += 1
    return bytes(out[:n])


# name -> (kind, size). Sizes cross the 64 KiB read buffer for one file.
ARTEFACTS = {
    "fid-1.4.0-x86_64.msi": ("installer", 70000),
    "fid-1.4.0-aarch64.dmg": ("installer", 1500),
    "fid-1.4.0-x86_64.AppImage": ("installer", 333),
    "fid-1.4.0+src.tar.gz": ("source", 65536),
}
SBOM = "fid-1.4.0-x86_64.msi.spdx.json"
NOT_IN_SUMS = [("SHA256SUMS", "checksums"), ("SHA256SUMS.dsse.json", "signature"), (SBOM, "sbom")]


def manifest(names):
    arts = []
    for name in names:
        kind = ARTEFACTS[name][0] if name in ARTEFACTS else "installer"
        arts.append({"name": name, "kind": kind, "url": "https://example.invalid/" + name})
    for name, kind in NOT_IN_SUMS:
        arts.append({"name": name, "kind": kind, "url": "https://example.invalid/" + name})
    doc = {"schema_version": 1, "version": VERSION, "channel": "stable", "artefacts": arts}
    return json.dumps(doc, indent=2) + "\n"


def write_case(case, files, listed):
    d = HERE / case
    if d.exists():
        shutil.rmtree(d)
    d.mkdir()
    for name in files:
        (d / name).write_bytes(dummy(name, ARTEFACTS[name][1] if name in ARTEFACTS else 64))
    # An SBOM next to the artefacts, as in a real release directory; it is not summed.
    (d / SBOM).write_bytes(b'{"spdxVersion":"SPDX-2.3"}\n')
    (d / "release-manifest.json").write_text(manifest(listed), encoding="utf-8", newline="\n")


names = list(ARTEFACTS)
write_case("ok", names, names)
write_case("extra-file", names + ["notes.txt"], names)
write_case("missing-file", names[:3], names)
write_case("bad-name", names, names + ["a b.msi"])

# Expected outputs for `ok`.
rows = []
for name in sorted(ARTEFACTS, key=lambda s: s.encode()):
    data = (HERE / "ok" / name).read_bytes()
    rows.append((name, hashlib.sha256(data).hexdigest(), len(data)))
exp = HERE / "expected"
exp.mkdir(exist_ok=True)
(exp / "SHA256SUMS").write_bytes("".join(f"{h}  {n}\n" for n, h, _ in rows).encode("ascii"))
statement = {"release": VERSION, "artefacts": [{"name": n, "sha256": h, "size": str(s)} for n, h, s in rows]}
canon = json.dumps(statement, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
(exp / "sums-statement.json").write_bytes(canon.encode("utf-8"))
print("wrote", len(rows), "artefacts")
