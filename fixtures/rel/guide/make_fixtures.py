#!/usr/bin/env python3
"""Independent reference for ECO.REL.SIGN.GUIDE fixtures (CC-BY-4.0 project data).

Writes, with the Python standard library only (hashlib, base64), never the Rust code:

- keys/*.pem: FAKE public keys. Each is a well-formed Ed25519 SubjectPublicKeyInfo DER
  (RFC 8410 prefix 302a300506032b6570032100 plus 32 bytes) whose 32 "key" bytes are the
  ASCII text FAKE-...-NOT-A-KEY. No private key exists for any of them and none was ever
  generated: they exist only so `cargo xtask rel keys-check` has files to fingerprint.
- keys/README.md: skipped by the check, as in the real keys/ directory.
- docs/KEYS.md: lists every fixture key with class, fingerprint (lowercase hex SHA-256 of the
  SPKI DER, computed here with hashlib), validity dates and status. keys-check exits 0.
- docs/KEYS-unlisted.md: the packaging key's row is missing (KEYS-UNLISTED).
- docs/KEYS-fingerprint.md: the nightly key's fingerprint has its last hex digit changed
  (KEYS-FINGERPRINT).
- docs/KEYS-expired.md: the old release key is still `active` after its validity ended
  (KEYS-EXPIRED-IN-USE when checked with --date 2026-11-18).
- release/: a tiny fixture release (two dummy artefacts with NUL bytes so git keeps them
  binary) and its SHA256SUMS (`<64 lowercase hex>  <name>`, sorted by name, LF). It is NOT
  signed: no signing key exists yet, so there is no SHA256SUMS.sig.

Run:  python fixtures/rel/guide/make_fixtures.py          (write)
      python fixtures/rel/guide/make_fixtures.py --check  (verify, write nothing)
"""

import base64
import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ED25519_SPKI_PREFIX = bytes.fromhex("302a300506032b6570032100")

# file -> (class, fake 32-byte key text, valid from, valid until, status)
KEYS = {
    "nightly-test.pem": ("nightly", b"FAKE-NIGHTLY-FIXTURE--NOT-A-KEY!", "2026-01-01", "2099-12-31", "active"),
    "packaging-test.pem": ("packaging", b"FAKE-PACKAGING-FIXTURE-NOT-A-KEY", "2026-01-01", "2099-12-31", "active"),
    "release-old-test.pem": ("release", b"FAKE-OLD-RELEASE-FIXTURE-NOT-KEY", "2020-01-01", "2021-12-31", "retired"),
    "release-test.pem": ("release", b"FAKE-RELEASE-FIXTURE--NOT-A-KEY!", "2026-01-01", "2099-12-31", "active"),
}

KEYS_README = """# Fixture keys (FAKE)

These are not keys. Each PEM file wraps 32 bytes of ASCII text in an Ed25519
SubjectPublicKeyInfo structure so that `cargo xtask rel keys-check` has something to
fingerprint. No private key exists for any of them. Written by ../make_fixtures.py.
"""


def spki_der(raw):
    assert len(raw) == 32, raw
    return ED25519_SPKI_PREFIX + raw


def pem(der):
    b64 = base64.b64encode(der).decode("ascii")
    lines = [b64[i : i + 64] for i in range(0, len(b64), 64)]
    return (
        "FAKE FIXTURE PUBLIC KEY: not a usable key, no private key exists.\n"
        "-----BEGIN PUBLIC KEY-----\n" + "\n".join(lines) + "\n-----END PUBLIC KEY-----\n"
    )


def fingerprint(der):
    return hashlib.sha256(der).hexdigest()


def keys_doc(rows, title):
    out = [
        f"# {title}",
        "",
        "Fixture for `cargo xtask rel keys-check` (ECO.REL.SIGN.GUIDE). FAKE keys only.",
        "",
        "| File | Class | Fingerprint | Valid from | Valid until | Status |",
        "|---|---|---|---|---|---|",
    ]
    for f, cls, fp, vf, vu, st in rows:
        out.append(f"| `{f}` | {cls} | `{fp}` | {vf} | {vu} | {st} |")
    out.append("")
    out.append("Text after the table is ignored.")
    return "\n".join(out) + "\n"


def base_rows():
    rows = []
    for f in sorted(KEYS):
        cls, raw, vf, vu, st = KEYS[f]
        rows.append([f, cls, fingerprint(spki_der(raw)), vf, vu, st])
    return rows


def dummy(name, n):
    """Deterministic dummy bytes, NUL included so git keeps them binary."""
    seed = name.encode()
    out = bytearray(b"\x00fixture artefact " + seed + b"\n")
    i = 0
    while len(out) < n:
        out += hashlib.sha256(seed + i.to_bytes(4, "big")).digest()
        i += 1
    return bytes(out[:n])


ARTEFACTS = {"fid-0.0.0-fixture+src.tar.gz": 2048, "fid-0.0.0-fixture-x86_64.AppImage": 777}


def outputs():
    files = {}
    for f, (_, raw, _, _, _) in KEYS.items():
        files[f"keys/{f}"] = pem(spki_der(raw)).encode("ascii")
    files["keys/README.md"] = KEYS_README.encode("ascii")

    rows = base_rows()
    files["docs/KEYS.md"] = keys_doc(rows, "Fixture KEYS.md (valid)").encode("ascii")

    unlisted = [r for r in rows if r[0] != "packaging-test.pem"]
    files["docs/KEYS-unlisted.md"] = keys_doc(unlisted, "Fixture KEYS.md (packaging key unlisted)").encode("ascii")

    wrong = [list(r) for r in rows]
    for r in wrong:
        if r[0] == "nightly-test.pem":
            r[2] = r[2][:-1] + ("0" if r[2][-1] != "0" else "1")
    files["docs/KEYS-fingerprint.md"] = keys_doc(wrong, "Fixture KEYS.md (wrong nightly fingerprint)").encode("ascii")

    expired = [list(r) for r in rows]
    for r in expired:
        if r[0] == "release-old-test.pem":
            r[5] = "active"
    files["docs/KEYS-expired.md"] = keys_doc(expired, "Fixture KEYS.md (expired key still active)").encode("ascii")

    sums = []
    for name in sorted(ARTEFACTS, key=lambda s: s.encode()):
        data = dummy(name, ARTEFACTS[name])
        files[f"release/{name}"] = data
        sums.append(f"{hashlib.sha256(data).hexdigest()}  {name}\n")
    files["release/SHA256SUMS"] = "".join(sums).encode("ascii")
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(outputs().items()):
        p = HERE / rel
        if check:
            if not p.is_file() or p.read_bytes() != data:
                stale.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if stale:
        print("guide fixtures stale: " + ", ".join(stale))
        sys.exit(1)
    print("guide fixtures: ok" if check else "guide fixtures: written")


if __name__ == "__main__":
    main()
