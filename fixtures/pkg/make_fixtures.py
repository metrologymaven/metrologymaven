#!/usr/bin/env python3
"""Independent reference for TRUST.PKG.MANIFEST (crate fid-pkg).

Writes every fixture under fixtures/pkg/ from the leaf spec's rules, without
running any Rust code:

- contents/*.json      synthetic member lists (inputs);
- golden/*             expected manifest.json / statement.json bytes, the minimal
                       container itself and pinned container SHA-256 digests;
- paths/good.txt, bad.txt  path-rule cases, labelled by this script's own validator;
- versions/*.json      manifests for the version-compatibility rule.

The ZIP container is assembled here byte by byte with `struct`, the canonical
JSON with a small RFC 8785 writer for string-only values, CRC-32 with `zlib` and
SHA-256 with `hashlib`. CC-BY-4.0 project data. Stdlib only.

Run:  python fixtures/pkg/make_fixtures.py          (writes the fixtures)
      python fixtures/pkg/make_fixtures.py --check  (fails if anything would change)
"""

import hashlib
import json
import struct
import sys
import unicodedata
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent

FORMAT_NAME = "metrologymaven-result-package"
FORMAT_VERSION = "1.0.0"
STATEMENT_TYPE = "https://in-toto.io/Statement/v1"
PREDICATE_TYPE = "https://metrologymaven.io/result-package/predicate/v1"
RESERVED_DIRS = ["interp", "journal", "kernel", "points", "profile", "results"]
STORED_MEDIA_TYPES = {
    "application/gzip",
    "application/x-bzip2",
    "application/x-xz",
    "application/zip",
    "application/zstd",
    "image/jpeg",
    "image/png",
}
MAX_PATH_BYTES = 4096


def sha256_hex(b):
    return hashlib.sha256(b).hexdigest()


# --------------------------------------------------------------- canonical JSON
# Only strings, objects and arrays occur in the package JSON (lengths and counts are
# decimal strings), so RFC 8785 reduces to: sorted keys (all keys are ASCII, so
# UTF-16 order equals code-point order), no whitespace, minimal string escaping.

def jstr(s):
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\t":
            out.append("\\t")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\r":
            out.append("\\r")
        elif o < 0x20:
            out.append("\\u%04x" % o)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def canon(v):
    if isinstance(v, str):
        return jstr(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jstr(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(f"unexpected JSON value {v!r}")


def canon_bytes(v):
    return canon(v).encode("utf-8")


# --------------------------------------------------------------- path rules

def path_ok(p):
    """REQ-TRUST.PKG.MANIFEST-006, plus: no empty segment (so no leading,
    trailing or doubled `/`)."""
    b = p.encode("utf-8", "surrogatepass")
    if not b or len(b) > MAX_PATH_BYTES:
        return False
    if any(0xD800 <= ord(c) <= 0xDFFF for c in p):
        return False
    if "\\" in p or "\x00" in p:
        return False
    if p.startswith("/"):
        return False
    if len(p) >= 2 and p[1] == ":" and p[0].isascii() and p[0].isalpha():
        return False
    if unicodedata.normalize("NFC", p) != p:
        return False
    for seg in p.split("/"):
        if seg in ("", ".", ".."):
            return False
    return True


# --------------------------------------------------------------- ZIP writer

DOS_TIME = 0x0000          # 00:00:00
DOS_DATE = (0 << 9) | (1 << 5) | 1  # 1980-01-01
FLAG_UTF8 = 0x0800
U32_MAX = 0xFFFFFFFF
U16_MAX = 0xFFFF
# "Version made by": Unix host (3) so tools neither convert names from an OEM code page
# nor drop permissions; spec version 4.5. Every entry is a regular file with mode 0644.
MADE_BY = (3 << 8) | 45
EXTERNAL_ATTR = 0o100644 << 16


def stored_deflate(data):
    """Deflate stream made only of stored blocks (RFC 1951 BTYPE=00), at most
    65535 bytes per block; an empty input is one empty final block."""
    if not data:
        return b"\x01\x00\x00\xff\xff"
    out = bytearray()
    for i in range(0, len(data), 65535):
        chunk = data[i:i + 65535]
        final = 1 if i + 65535 >= len(data) else 0
        out += bytes([final]) + struct.pack("<HH", len(chunk), len(chunk) ^ 0xFFFF) + chunk
    return bytes(out)


def method_for(media_type):
    return 0 if media_type in STORED_MEDIA_TYPES else 8


def build_zip(entries, force_zip64=False):
    """entries: list of (name, media_type, data). Returns the container bytes."""
    out = bytearray()
    central = []
    for name, media_type, data in entries:
        nb = name.encode("utf-8")
        method = method_for(media_type)
        comp = data if method == 0 else stored_deflate(data)
        crc = zlib.crc32(data) & 0xFFFFFFFF
        offset = len(out)
        sizes64 = force_zip64 or len(data) >= U32_MAX or len(comp) >= U32_MAX
        off64 = force_zip64 or offset >= U32_MAX
        need = 45 if (sizes64 or off64) else 20
        local_extra = struct.pack("<HHQQ", 1, 16, len(data), len(comp)) if sizes64 else b""
        out += struct.pack("<IHHHHHIIIHH", 0x04034B50, need, FLAG_UTF8, method, DOS_TIME, DOS_DATE, crc,
                           U32_MAX if sizes64 else len(comp), U32_MAX if sizes64 else len(data),
                           len(nb), len(local_extra))
        out += nb + local_extra + comp
        cext = b""
        if sizes64:
            cext += struct.pack("<QQ", len(data), len(comp))
        if off64:
            cext += struct.pack("<Q", offset)
        if cext:
            cext = struct.pack("<HH", 1, len(cext)) + cext
        central.append(struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, MADE_BY, need, FLAG_UTF8, method, DOS_TIME,
                                   DOS_DATE, crc, U32_MAX if sizes64 else len(comp),
                                   U32_MAX if sizes64 else len(data), len(nb), len(cext), 0, 0, 0, EXTERNAL_ATTR,
                                   U32_MAX if off64 else offset) + nb + cext)
    cd_offset = len(out)
    for c in central:
        out += c
    cd_size = len(out) - cd_offset
    count = len(entries)
    eocd64 = force_zip64 or count >= U16_MAX or cd_size >= U32_MAX or cd_offset >= U32_MAX
    if eocd64:
        rec_offset = len(out)
        out += struct.pack("<IQHHIIQQQQ", 0x06064B50, 44, 45, 45, 0, 0, count, count, cd_size, cd_offset)
        out += struct.pack("<IIQI", 0x07064B50, 0, rec_offset, 1)
        out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, U16_MAX, U16_MAX, U32_MAX, U32_MAX, 0)
    else:
        out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, count, count, cd_size, cd_offset, 0)
    return bytes(out)


# --------------------------------------------------------------- package

def body_of(m):
    if "body" in m:
        return m["body"].encode("utf-8")
    text, count = m["body_repeat"]
    return (text * count).encode("utf-8")


def build_package(contents, force_zip64=False):
    members = []
    for m in contents["members"]:
        assert path_ok(m["path"]), m["path"]
        assert m["path"].split("/")[0] in RESERVED_DIRS and "/" in m["path"], m["path"]
        members.append((m["path"], m["media_type"], body_of(m)))
    members.sort(key=lambda t: t[0].encode("utf-8"))
    paths = [t[0] for t in members]
    assert len(set(paths)) == len(paths)
    assert len({p.lower() for p in paths}) == len(paths), "paths collide after case folding"
    manifest = {
        "format": FORMAT_NAME,
        "format_version": FORMAT_VERSION,
        "created_by": contents["created_by"],
        "evidence_tier": contents["evidence_tier"],
        "profile_digest": contents["profile_digest"],
        "kernel_manifest_digest": contents["kernel_manifest_digest"],
        "members": [{"path": p, "media_type": mt, "length": str(len(d)), "sha256": sha256_hex(d)}
                    for p, mt, d in members],
    }
    mbytes = canon_bytes(manifest)
    statement = {
        "_type": STATEMENT_TYPE,
        "subject": [{"name": "manifest.json", "digest": {"sha256": sha256_hex(mbytes)}}]
        + [{"name": p, "digest": {"sha256": sha256_hex(d)}} for p, _, d in members],
        "predicateType": PREDICATE_TYPE,
        "predicate": {
            "manifest_sha256": sha256_hex(mbytes),
            "format_version": FORMAT_VERSION,
            "result_count": str(contents["result_count"]),
            "evidence_tier": contents["evidence_tier"],
        },
    }
    sbytes = canon_bytes(statement)
    entries = [("manifest.json", "application/json", mbytes)] + members + [
        ("statement.json", "application/json", sbytes)]
    return mbytes, sbytes, build_zip(entries, force_zip64)


# --------------------------------------------------------------- fixture data

def digest_of(text):
    return "sha256:" + sha256_hex(text.encode("utf-8"))


PROFILE_BODY = '{"profile":"synthetic-test-profile","rules":[]}'
KERNEL_BODY = '{"kernel":"synthetic-test-kernel","version":"0.0.0"}'

MINIMAL = {
    "created_by": "fid-pkg fixture generator 1",
    "evidence_tier": "results",
    "profile_digest": digest_of(PROFILE_BODY),
    "kernel_manifest_digest": digest_of(KERNEL_BODY),
    "result_count": 2,
    "members": [
        {"path": "results/records.json", "media_type": "application/json",
         "body": '[{"id":"r1","value":"0x3ff0000000000000"},{"id":"r2","value":"0x4000000000000000"}]'},
        {"path": "profile/profile.json", "media_type": "application/json", "body": PROFILE_BODY},
        {"path": "kernel/manifest.json", "media_type": "application/json", "body": KERNEL_BODY},
    ],
}

MIXED = {
    "created_by": "fid-pkg fixture generator 1 \u00e9\u00e8 \"quoted\"\ttab",
    "evidence_tier": "points",
    "profile_digest": digest_of(PROFILE_BODY),
    "kernel_manifest_digest": digest_of(KERNEL_BODY),
    "result_count": 1,
    "members": [
        {"path": "results/results.qif", "media_type": "application/xml", "body": "<QIFDocument/>\n"},
        {"path": "points/" + "ab" * 32 + ".zst", "media_type": "application/zstd",
         "body": "\x28\xb5\x2f\xfd not really zstd"},
        {"path": "journal/segment.jsonl", "media_type": "application/x-ndjson",
         "body_repeat": ['{"seq":"1","event":"synthetic"}\n', 2200]},
        {"path": "journal/checkpoints/0.json", "media_type": "application/json", "body": ""},
        {"path": "results/caf\u00e9 \u00fc\u00dfer.json", "media_type": "application/json", "body": "{}"},
        {"path": "profile/profile.json", "media_type": "application/json", "body": PROFILE_BODY},
        {"path": "kernel/manifest.json", "media_type": "application/json", "body": KERNEL_BODY},
    ],
}

GOOD_PATHS = [
    "results/records.json",
    "a",
    "a/b/c/d.txt",
    "points/" + "0f" * 32 + ".e57",
    "caf\u00e9/na\u00efve.json",
    "\u65e5\u672c/\u30c7\u30fc\u30bf.json",
    "with space/and-dash_and.dots.json",
    "..a/b..",
    ".hidden/x",
    "ab:c/d",
    "1:/x",
    "x" * MAX_PATH_BYTES,
    "\u00e9" * (MAX_PATH_BYTES // 2),
]

BAD_PATHS = [
    "",
    "../x",
    "a/../b",
    "a/..",
    "./a",
    "a/./b",
    "a/.",
    ".",
    "..",
    "a\\b",
    "\\a",
    "/abs",
    "/",
    "//server/share",
    "C:x",
    "c:/x",
    "Z:",
    "a:b/c",
    "a\u0000b",
    "\u0000",
    "cafe\u0301.json",
    "results/e\u0301",
    "A\u030a",
    "a//b",
    "a/",
    "x" * (MAX_PATH_BYTES + 1),
    "\u00e9" * (MAX_PATH_BYTES // 2) + "x",
]

V1_4 = {
    "format": FORMAT_NAME,
    "format_version": "1.4.0",
    "created_by": "a future 1.x writer",
    "evidence_tier": "features",
    "profile_digest": digest_of(PROFILE_BODY),
    "kernel_manifest_digest": digest_of(KERNEL_BODY),
    "future_optional_field": "ignored by a 1.0 reader",
    "members": [{"path": "results/records.json", "media_type": "application/json", "length": "2",
                 "sha256": sha256_hex(b"[]")}],
}
V2_0 = dict(V1_4, format_version="2.0.0", created_by="a future 2.x writer")


# --------------------------------------------------------------- output

def write_all():
    files = {}

    def put(rel, data):
        files[rel] = data if isinstance(data, bytes) else data.encode("utf-8")

    put("contents/minimal.json", json.dumps(MINIMAL, indent=2, ensure_ascii=True) + "\n")
    put("contents/mixed.json", json.dumps(MIXED, indent=2, ensure_ascii=True) + "\n")

    for name, contents in (("minimal", MINIMAL), ("mixed", MIXED)):
        mbytes, sbytes, zbytes = build_package(contents)
        put(f"golden/{name}.manifest.json", mbytes)
        put(f"golden/{name}.statement.json", sbytes)
        put(f"golden/{name}.zip.sha256", f"{sha256_hex(zbytes)}  {name}.zip\n")
        if name == "minimal":
            put("golden/minimal.zip", zbytes)
        _, _, z64 = build_package(contents, force_zip64=True)
        put(f"golden/{name}.zip64.zip.sha256", f"{sha256_hex(z64)}  {name}.zip64.zip\n")

    for p in GOOD_PATHS:
        assert path_ok(p), p
    for p in BAD_PATHS:
        assert not path_ok(p), p
    header = "# One JSON string literal per line; lines starting with # are comments.\n"
    put("paths/good.txt", header + "".join(json.dumps(p, ensure_ascii=True) + "\n" for p in GOOD_PATHS))
    put("paths/bad.txt", header + "".join(json.dumps(p, ensure_ascii=True) + "\n" for p in BAD_PATHS))

    put("versions/v1_4_0.json", canon_bytes(V1_4))
    put("versions/v2_0_0.json", canon_bytes(V2_0))
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(write_all().items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures:", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
