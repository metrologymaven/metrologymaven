#!/usr/bin/env python3
"""Independent reference for PLAT.DEPLOY.OFFLINE.BUNDLE: writes the miniature offline bundles,
the install tree, the package plan, the bad install manifest and the two exported JSON Schemas
used by the fid-platform tests.

Every file below is hand-written here; digests and sizes are computed with hashlib from those
bytes, never by running or reading the Rust crate. JSON is RFC 8785 canonical: every value is
an ASCII string, an integer or a structure of those, so sorted keys and compact separators are
enough. Manifests carry no trailing newline (the reader requires canonical bytes); the two
schema files end with one newline.

Expected outcomes (hand-labelled from NODE.md):
  bundle-ok/            check-bundle exits 0 (AC-1). Its docs hold an <a> link to https, a
                        commented-out remote <img>, relative url() and @import: none is an
                        auto-loaded remote resource.
  bundle-missing-cli/   no `cli` entry and no cli file: MissingRole { role: cli } (AC-2)
  bundle-tampered/      offline-bundle.json of bundle-ok, one installer byte changed:
                        DigestMismatch { path: installers/fid-report-0.1.0-windows-x86_64.msi } (AC-3)
  bundle-remote-font/   docs/style.css loads a web font over https (listed with correct digests):
                        RemoteResource { file: docs/style.css, url: https://fonts.example.com/mm.woff2 } (AC-4)
  install-manifest-dotdot.json   a `../x` relpath: BadPath (AC-5)
  install-tree/         install-manifest-per_user.json of bundle-ok is its manifest (AC-6)

Fixtures and this script: MIT OR Apache-2.0 (D-001).
Run:  python fixtures/plat/deploy/make_bundles.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
RELEASE = "0.1.0"
OS = "windows"
ARCH = "x86_64"
DIALECT = "https://json-schema.org/draft/2020-12/schema"
OS_ALL = ["windows", "ubuntu", "debian", "fedora", "rhel", "macos"]
ARCH_ALL = ["x86_64", "aarch64"]
ROLES = ["installer", "cli", "docs", "runtime_dep", "install_manifest", "licence"]
KINDS = ["per_user", "machine"]
ID_PATTERN = "^[a-z0-9][a-z0-9_.-]*$"
RELEASE_PATTERN = "^[0-9A-Za-z][0-9A-Za-z.+-]*$"
RELPATH_PATTERN = r"^[^/\\:\u0000-\u001f]+(/[^/\\:\u0000-\u001f]+)*$"
DOT_SEGMENT_PATTERN = r"(^|/)\.{1,2}(/|$)"
DIGEST_PATTERN = "^sha256:[0-9a-f]{64}$"
MAX_ENTRIES = 200000
MAX_PATH_BYTES = 1024
MAX_SIZE = 2**53 - 1

outputs = {}


def jcs(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def put(rel, data):
    if isinstance(data, str):
        data = data.encode("utf-8")
    outputs[rel] = data


# ---------------------------------------------------------------- install tree (AC-6)

TREE = {
    "bin/fid.exe": b"dummy fid binary\n",
    "bin/report.exe": b"dummy report app\n",
    "docs/index.html": b"<!doctype html>\n<title>Metrology Maven</title>\n<p>Offline docs.</p>\n",
    "share/README.txt": b"Installed by the offline bundle fixture.\n",
}


def install_manifest(kind):
    files = [{"relpath": p, "sha256": sha(b), "size": len(b)} for p, b in sorted(TREE.items())]
    return jcs({"schema_version": 1, "release": RELEASE, "os": OS, "arch": ARCH, "kind": kind, "files": files})


for p, b in TREE.items():
    put("install-tree/" + p, b)

# ---------------------------------------------------------------- package plan

PLAN = {
    "schema_version": 1,
    "version": RELEASE,
    "packages": [
        # Windows x86_64 (tier1): apps report and inspect, the CLI and fid-verify.
        {"package": "fid", "kind": "cli", "os": "windows", "arch": "x86_64", "tier": "tier1",
         "contents": ["fid"], "file_name": "fid-fid-0.1.0-windows-x86_64.zip"},
        {"package": "fid-verify", "kind": "verify", "os": "windows", "arch": "x86_64", "tier": "tier1",
         "contents": ["fid-verify"], "file_name": "fid-fid-verify-0.1.0-windows-x86_64.zip"},
        {"package": "inspect", "kind": "app", "os": "windows", "arch": "x86_64", "tier": "tier1",
         "contents": ["inspect"], "file_name": "fid-inspect-0.1.0-windows-x86_64.msi"},
        {"package": "report", "kind": "app", "os": "windows", "arch": "x86_64", "tier": "tier1",
         "contents": ["report"], "file_name": "fid-report-0.1.0-windows-x86_64.msi"},
        # Ubuntu aarch64 (tier2): not the bundle's combination, so ignored by every check here.
        {"package": "report", "kind": "app", "os": "ubuntu", "arch": "aarch64", "tier": "tier2",
         "contents": ["report"], "file_name": "fid-report-0.1.0-ubuntu-aarch64.deb"},
    ],
    "runtime_deps": [
        {"os": "windows", "arch": "x86_64", "file_name": "c-runtime-x86_64.exe"},
        {"os": "ubuntu", "arch": "aarch64", "file_name": "libfoo-aarch64.deb"},
    ],
}
put("pkg-plan.json", jcs(PLAN))

# ---------------------------------------------------------------- bundles

INDEX_OK = """<!doctype html>
<html><head>
<meta charset="utf-8">
<title>Metrology Maven docs</title>
<link rel="stylesheet" href="style.css">
<script src="search.js"></script>
</head><body>
<!-- <img src="https://example.org/banner.png"> is commented out and never loaded -->
<p>See <a href="https://example.org/">the project site</a> when online.</p>
<img src="img/logo.svg" alt="logo">
<p style="background: url(img/bg.svg)">Offline.</p>
</body></html>
"""
STYLE_OK = """@import "theme.css";
@font-face { font-family: mm; src: url("fonts/mm.woff2") format("woff2"); }
body { font-family: mm, sans-serif; }
/* url(https://example.org/old.woff2) was the old font, now bundled */
"""
STYLE_REMOTE = """@import "theme.css";
@font-face { font-family: mm; src: url("https://fonts.example.com/mm.woff2") format("woff2"); }
body { font-family: mm, sans-serif; }
"""
THEME = "p { color: #222; }\n"
SEARCH = """// Offline search over the bundled index.
const index = "search-index.json";
function search(q) { return q.length; }
"""
LOGO = '<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>\n'

BASE = {
    "installers/fid-inspect-0.1.0-windows-x86_64.msi": ("installer", b"dummy inspect installer\n"),
    "installers/fid-report-0.1.0-windows-x86_64.msi": ("installer", b"dummy report installer\n"),
    "cli/fid-fid-0.1.0-windows-x86_64.zip": ("cli", b"dummy cli archive\n"),
    "docs/index.html": ("docs", INDEX_OK.encode()),
    "docs/style.css": ("docs", STYLE_OK.encode()),
    "docs/theme.css": ("docs", THEME.encode()),
    "docs/search.js": ("docs", SEARCH.encode()),
    "docs/img/logo.svg": ("docs", LOGO.encode()),
    "install/install-manifest-machine.json": ("install_manifest", install_manifest("machine")),
    "install/install-manifest-per_user.json": ("install_manifest", install_manifest("per_user")),
    "runtime/c-runtime-x86_64.exe": ("runtime_dep", b"dummy c runtime\n"),
    "licences/LICENSE-MIT.txt": ("licence", b"MIT licence text placeholder for the fixture\n"),
}


def bundle_json(files, bundle_id):
    entries = [{"path": p, "role": r, "sha256": sha(b), "size": len(b)} for p, (r, b) in sorted(files.items())]
    return jcs({"schema_version": 1, "bundle_id": bundle_id, "release": RELEASE, "os": OS, "arch": ARCH,
                "entries": entries})


def write_bundle(name, files, listing=None):
    for p, (_, b) in files.items():
        put(f"{name}/{p}", b)
    put(f"{name}/offline-bundle.json", listing if listing is not None else bundle_json(files, name))


write_bundle("bundle-ok", BASE)

missing_cli = {p: v for p, v in BASE.items() if v[0] != "cli"}
write_bundle("bundle-missing-cli", missing_cli)

tampered = dict(BASE)
victim = "installers/fid-report-0.1.0-windows-x86_64.msi"
role, data = tampered[victim]
tampered[victim] = (role, data[:0] + b"D" + data[1:])  # 'd' -> 'D': one byte differs
write_bundle("bundle-tampered", tampered, listing=bundle_json(BASE, "bundle-tampered"))

remote = dict(BASE)
remote["docs/style.css"] = ("docs", STYLE_REMOTE.encode())
write_bundle("bundle-remote-font", remote)

# ---------------------------------------------------------------- AC-5

put("install-manifest-dotdot.json", jcs({
    "schema_version": 1, "release": RELEASE, "os": OS, "arch": ARCH, "kind": "per_user",
    "files": [{"relpath": "../x", "sha256": sha(b"x"), "size": 1}],
}))

# ---------------------------------------------------------------- exported JSON Schemas


def closed(required, props):
    return {"type": "object", "additionalProperties": False, "required": required, "properties": props}


PATH_SCHEMA = {"type": "string", "minLength": 1, "maxLength": MAX_PATH_BYTES, "pattern": RELPATH_PATTERN,
               "not": {"pattern": DOT_SEGMENT_PATTERN}}
DIGEST_SCHEMA = {"type": "string", "pattern": DIGEST_PATTERN}
SIZE_SCHEMA = {"type": "integer", "minimum": 0, "maximum": MAX_SIZE}
RELEASE_SCHEMA = {"type": "string", "maxLength": 64, "pattern": RELEASE_PATTERN}


def top(title, required, props):
    d = {"$schema": DIALECT, "title": title}
    d.update(closed(required, props))
    return d


BUNDLE_SCHEMA = top(
    "Metrology Maven offline bundle, schema_version 1 (PLAT.DEPLOY.OFFLINE.BUNDLE)",
    ["schema_version", "bundle_id", "release", "os", "arch", "entries"],
    {
        "schema_version": {"const": 1},
        "bundle_id": {"type": "string", "maxLength": 128, "pattern": ID_PATTERN},
        "release": RELEASE_SCHEMA,
        "os": {"enum": OS_ALL},
        "arch": {"enum": ARCH_ALL},
        "entries": {"type": "array", "maxItems": MAX_ENTRIES, "items": closed(
            ["path", "role", "sha256", "size"],
            {"path": PATH_SCHEMA, "role": {"enum": ROLES}, "sha256": DIGEST_SCHEMA, "size": SIZE_SCHEMA},
        )},
    },
)
INSTALL_SCHEMA = top(
    "Metrology Maven install manifest, schema_version 1 (PLAT.DEPLOY.OFFLINE.BUNDLE)",
    ["schema_version", "release", "os", "arch", "kind", "files"],
    {
        "schema_version": {"const": 1},
        "release": RELEASE_SCHEMA,
        "os": {"enum": OS_ALL},
        "arch": {"enum": ARCH_ALL},
        "kind": {"enum": KINDS},
        "files": {"type": "array", "maxItems": MAX_ENTRIES, "items": closed(
            ["relpath", "sha256", "size"],
            {"relpath": PATH_SCHEMA, "sha256": DIGEST_SCHEMA, "size": SIZE_SCHEMA},
        )},
    },
)
schema_outputs = {
    "platform/offline-bundle.schema.json": jcs(BUNDLE_SCHEMA) + b"\n",
    "platform/install-manifest.schema.json": jcs(INSTALL_SCHEMA) + b"\n",
}


def validate_against_schemas():
    """REQ-005: every fixture document validates against its exported schema, and the AC-5
    manifest does not. Skipped (with a note) when the jsonschema package is not installed."""
    try:
        import jsonschema
    except ImportError:
        print("note: jsonschema not installed, schema validation skipped")
        return 0
    v = {k: jsonschema.Draft202012Validator(json.loads(d)) for k, d in schema_outputs.items()}
    bad = 0
    for rel, data in outputs.items():
        if not rel.endswith(".json") or rel == "pkg-plan.json":
            continue
        which = "platform/offline-bundle.schema.json" if rel.endswith("offline-bundle.json") else             "platform/install-manifest.schema.json"
        valid = v[which].is_valid(json.loads(data))
        if valid == rel.endswith("dotdot.json"):
            print(f"schema: {rel} is {'valid' if valid else 'invalid'} against {which}")
            bad += 1
    return bad


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    targets = [(HERE / rel, data) for rel, data in outputs.items()]
    targets += [(ROOT / rel, data) for rel, data in schema_outputs.items()]
    for path, data in targets:
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        bad += validate_against_schemas()
        print("ok" if bad == 0 else f"{bad} problem(s)")
        return 1 if bad else 0
    print(f"wrote {len(targets)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
