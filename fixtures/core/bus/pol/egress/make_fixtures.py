#!/usr/bin/env python3
"""Generate fixtures/core/bus/pol/egress/artifact.bin and cases.json (CORE.BUS.POL.EGRESS).

artifact.bin is 4099 pseudo-random bytes from a fixed seed: SHA-256 in counter mode over
b"CORE.BUS.POL.EGRESS artifact" plus a 4-byte big-endian counter. Its digest and size in
cases.json are computed here with hashlib, independently of the Rust code.

cases.json also holds hand-labelled cases, stated from the rules of
spec/core/bus/pol/egress/NODE.md and the strict destination grammar of
crates/fid-bus/src/policy/egress.rs (module documentation): destination IDs, https URLs
(accept or refuse), whole destination settings (accept or refuse), and the expected
configuration digests of the accepted destinations (sha256 of the RFC 8785 canonical JSON of
{id, kind, path} or {id, kind, method, url}; ASCII strings only, so sorted-key compact
json.dumps is RFC 8785 canonical). CC-BY-4.0 project data.

Run:    python fixtures/core/bus/pol/egress/make_fixtures.py
Check:  python fixtures/core/bus/pol/egress/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SEED = b"CORE.BUS.POL.EGRESS artifact"
SIZE = 4099


def artifact():
    out = b""
    i = 0
    while len(out) < SIZE:
        out += hashlib.sha256(SEED + i.to_bytes(4, "big")).digest()
        i += 1
    return out[:SIZE]


def canonical(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# Destination IDs: lowercase ASCII letters and digits, '-' and '_', first a letter or digit,
# 1 to 64 bytes. Hand-labelled.
IDS = [
    ("ok", "lab-share", True),
    ("ok", "a", True),
    ("ok", "0_x-y", True),
    ("ok", "a" * 64, True),
    ("too long", "a" * 65, False),
    ("empty", "", False),
    ("uppercase", "Lab-share", False),
    ("leading dash", "-lab", False),
    ("leading underscore", "_lab", False),
    ("space", "lab share", False),
    ("dot", "lab.share", False),
    ("Cyrillic a lookalike", "l\u0430b", False),
    ("fullwidth letter", "\uff4cab", False),
    ("zero width joiner", "lab\u200dshare", False),
    ("dotless i", "\u0131d", False),
]

# https URLs, hand-labelled from the strict form.
URLS = [
    ("plain", "https://upload.example.org/inbox", True),
    ("port", "https://upload.example.org:8443/inbox/", True),
    ("ipv4", "https://192.0.2.10/put", True),
    ("single label", "https://localhost/x", True),
    ("unreserved path", "https://a.example/a-b_c.d~e/f", True),
    ("root path", "https://a.example/", True),
    ("http", "http://upload.example.org/inbox", False),
    ("uppercase scheme", "HTTPS://upload.example.org/inbox", False),
    ("uppercase host", "https://Upload.example.org/inbox", False),
    ("no path", "https://upload.example.org", False),
    ("user info", "https://user@upload.example.org/inbox", False),
    ("user info lookalike", "https://upload.example.org@evil.example/inbox", False),
    ("query", "https://upload.example.org/inbox?x=1", False),
    ("fragment", "https://upload.example.org/inbox#f", False),
    ("percent escape", "https://upload.example.org/in%62ox", False),
    ("percent in host", "https://upload%2eexample.org/inbox", False),
    ("backslash", "https://upload.example.org\\@evil.example/inbox", False),
    ("idn punycode", "https://xn--pple-43d.com/inbox", False),
    ("reserved label", "https://ab--cd.example/inbox", False),
    ("non-ascii host", "https://\u0430pple.com/inbox", False),
    ("fullwidth dot", "https://upload\uff0eexample.org/inbox", False),
    ("trailing dot", "https://upload.example.org./inbox", False),
    ("empty label", "https://upload..example.org/inbox", False),
    ("label leading dash", "https://-upload.example.org/inbox", False),
    ("underscore host", "https://up_load.example.org/inbox", False),
    ("ipv6", "https://[2001:db8::1]/inbox", False),
    ("decimal ip", "https://3221225994/inbox", False),
    ("short ip", "https://192.0.2/inbox", False),
    ("octal ip", "https://0300.0.2.10/inbox", False),
    ("hex ip", "https://0xc0.0.2.10/inbox", False),
    ("ip over 255", "https://192.0.2.256/inbox", False),
    ("numeric last label", "https://example.123/inbox", False),
    ("port zero", "https://a.example:0/x", False),
    ("port leading zero", "https://a.example:0443/x", False),
    ("port too big", "https://a.example:65536/x", False),
    ("empty port", "https://a.example:/x", False),
    ("dot segment", "https://a.example/a/./b", False),
    ("dot dot segment", "https://a.example/a/../b", False),
    ("empty segment", "https://a.example/a//b", False),
    ("space", "https://a.example/a b", False),
    ("tab", "https://a.example/a\tb", False),
    ("label 64 bytes", "https://" + "a" * 64 + ".example/x", False),
    ("label 63 bytes", "https://" + "a" * 63 + ".example/x", True),
    ("NUL", "https://a.example/a\u0000b", False),
    ("DEL", "https://a.example/a\u007fb", False),
    ("two ports", "https://a.example:1:2/x", False),
    ("2048 bytes", "https://a.example/" + "p" * (2048 - len("https://a.example/")), True),
    ("2049 bytes", "https://a.example/" + "p" * (2049 - len("https://a.example/")), False),
]

# network_root paths, hand-labelled: absolute, no '.' or '..' segment, no control character,
# no Windows device or verbatim prefix; a drive or a UNC share on Windows.
ROOT_PATHS = [
    ("unix absolute", "unix", "/srv/egress/lab", True),
    ("unix relative", "unix", "srv/egress", False),
    ("unix dot dot", "unix", "/srv/../etc", False),
    ("unix dot", "unix", "/srv/./lab", False),
    ("unix trailing dot dot", "unix", "/srv/lab/..", False),
    ("unix NUL", "unix", "/srv/a\u0000b", False),
    ("unix newline", "unix", "/srv/a\nb", False),
    ("unix backslash dot dot", "unix", "/srv\\..\\etc", False),
    ("windows drive", "windows", "C:\\egress\\lab", True),
    ("windows UNC share", "windows", "\\\\server\\share\\inbox", True),
    ("windows drive relative", "windows", "C:egress", False),
    ("windows dot dot", "windows", "C:\\egress\\..\\x", False),
    ("windows forward dot dot", "windows", "C:/egress/../x", False),
    ("windows verbatim", "windows", "\\\\?\\C:\\egress", False),
    ("windows verbatim UNC", "windows", "\\\\?\\UNC\\server\\share", False),
    ("windows device", "windows", "\\\\.\\COM1", False),
    ("windows device slashes", "windows", "//./pipe/x", False),
    ("windows NT object", "windows", "\\??\\C:\\egress", False),
    ("windows DEL", "windows", "C:\\a\u007fb", False),
]

LAB = {"id": "lab-share", "kind": "network_root", "path": "/srv/egress/lab"}
WIN_LAB = {"id": "lab-share", "kind": "network_root", "path": "C:\\egress\\lab"}
UPLOAD = {"id": "upload", "kind": "https", "url": "https://upload.example.org/inbox", "method": "PUT"}

# Whole setting values. Network root paths must be absolute on the platform running the
# test, so each case names the platforms it applies to ("unix", "windows" or "all").
SETTINGS = [
    ("empty", "all", [], True),
    ("unix root and https", "unix", [LAB, UPLOAD], True),
    ("windows root and https", "windows", [WIN_LAB, UPLOAD], True),
    ("post method", "all", [dict(UPLOAD, method="POST")], True),
    ("not an array", "all", {"id": "x"}, False),
    ("duplicate id", "all", [UPLOAD, dict(UPLOAD, url="https://other.example/inbox")], False),
    ("unknown field", "all", [dict(UPLOAD, header="x")], False),
    ("path on https", "all", [dict(UPLOAD, path="/srv")], False),
    ("url on network root", "unix", [dict(LAB, url="https://a.example/x")], False),
    ("unknown kind", "all", [dict(UPLOAD, kind="ftp")], False),
    ("lowercase method", "all", [dict(UPLOAD, method="put")], False),
    ("GET method", "all", [dict(UPLOAD, method="GET")], False),
    ("missing method", "all", [{"id": "upload", "kind": "https", "url": "https://upload.example.org/inbox"}], False),
    ("bad url", "all", [dict(UPLOAD, url="http://upload.example.org/inbox")], False),
    ("relative path", "all", [dict(LAB, path="egress/lab")], False),
    ("uppercase id", "all", [dict(UPLOAD, id="Upload")], False),
    ("lookalike id", "all", [dict(UPLOAD, id="uplo\u0430d")], False),
    ("too many", "all", [dict(UPLOAD, id="d%d" % i) for i in range(65)], False),
    ("sixty-four", "all", [dict(UPLOAD, id="d%d" % i) for i in range(64)], True),
]


def config_digest(d):
    if d["kind"] == "network_root":
        v = {"id": d["id"], "kind": "network_root", "path": d["path"]}
    else:
        v = {"id": d["id"], "kind": "https", "method": d["method"], "url": d["url"]}
    return digest(canonical(v))


def build():
    art = artifact()
    cases = {
        "artifact": {"file": "artifact.bin", "digest": digest(art), "size": len(art)},
        "ids": [{"name": n, "id": i, "valid": ok} for n, i, ok in IDS],
        "urls": [{"name": n, "url": u, "valid": ok} for n, u, ok in URLS],
        "root_paths": [{"name": n, "platform": p, "path": v, "valid": ok} for n, p, v, ok in ROOT_PATHS],
        "settings": [{"name": n, "platform": p, "value": v, "valid": ok} for n, p, v, ok in SETTINGS],
        "config_digests": [{"destination": d, "digest": config_digest(d)} for d in (LAB, WIN_LAB, UPLOAD)],
    }
    text = json.dumps(cases, indent=2, ensure_ascii=True, sort_keys=True) + "\n"
    return {"artifact.bin": art, "cases.json": text.encode("ascii")}


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        bad = [n for n, b in files.items() if not (HERE / n).exists() or (HERE / n).read_bytes() != b]
        if bad:
            print("stale: " + ", ".join(bad))
            return 1
        print("ok")
        return 0
    for n, b in files.items():
        (HERE / n).write_bytes(b)
    return 0


if __name__ == "__main__":
    sys.exit(main())
