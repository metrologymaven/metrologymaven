#!/usr/bin/env python3
"""Independent reference for ECO.REL.SIGN.SBOM (CC-BY-4.0 project data).

Writes the fixtures under fixtures/rel/sbom/ and the expected SPDX 2.3 JSON documents, computed
here from the rules below and from the fixture definitions in this file (the dependency graph
per target is evaluated by hand from those definitions, not by running cargo or the Rust code).

Fixtures:
- ws/: a three-crate Cargo workspace. `fid-cli` (binary) depends on `fid-core`, on the vendored
  third-party crate `fid-sbom-fixture-dep` (MIT OR Apache-2.0, served offline through Cargo
  source replacement from ws/vendor/) and, on Windows only, on `fid-winio`. Cargo.lock is
  written here; the vendored crate's lockfile checksum is a stand-in (SHA-256 of its files).
- plan.json: two artefacts (Linux tarball, Windows zip) with their target triples, contents,
  the release version, the commit time and the artefact SHA-256 values. artefacts/ holds the
  two artefact files those digests are taken from.
- native-components.toml: one font assigned to package `fid` (both artefacts).
- native-bad-digest.toml: the same font with a malformed SHA-256 (SBOM-NATIVE-DIGEST).
- bad-licence/: a one-crate workspace whose licence is `GPL-2.0 OR` (SBOM-LICENCE), and its plan.
- expected/<artefact>.spdx.json: the documents `cargo xtask rel sbom` must write for plan.json.

Document rules (shared with crates/fid-xtask/src/rel/sbom.rs):
- JSON with keys sorted, no insignificant whitespace, UTF-8, no trailing newline;
- spdxVersion SPDX-2.3, dataLicense CC0-1.0, SPDXID SPDXRef-DOCUMENT, name = artefact file name;
- documentNamespace = REPO/spdx/<artefact>-<uuid>, uuid = UUIDv5(URL namespace,
  REPO/spdx/<version>/<artefact>);
- creationInfo: created = the plan's commit time, creators = ["Tool: fid-xtask-<workspace version>"];
- packages: the root package SPDXRef-Artefact first (name = artefact, versionInfo = release
  version, SHA256 checksum of the artefact, NOASSERTION elsewhere), then the components sorted by
  (name, version, purl, downloadLocation);
- crate component: SPDXID SPDXRef-Crate-<name>-<version>, native: SPDXRef-Native-<name>-<version>,
  every character outside [A-Za-z0-9.-] replaced by '-', and "-2", "-3", ... appended to a repeat;
  filesAnalyzed false, licenseConcluded and copyrightText NOASSERTION, licenseDeclared = the
  licence expression; one purl externalRef (pkg:cargo/<name>@<version> for crates,
  pkg:generic/<name>@<version> for native components, characters outside [A-Za-z0-9._~-]
  percent-encoded with uppercase hex; a path crate's purl ends in ?repository_url=REPO);
  checksums = [SHA256] from Cargo.lock or the native file,
  omitted when there is none; downloadLocation: crates.io download URL for a crates.io source,
  NOASSERTION for a path crate, the `source` URL for a native component;
- relationships: DOCUMENT DESCRIBES SPDXRef-Artefact, then SPDXRef-Artefact CONTAINS each
  component in package order (this fixture has no build dependency or proc-macro crate; the
  BUILD_DEPENDENCY_OF rule and the legacy `/` licence rule are covered by hand-labelled unit tests
  in sbom.rs).

Run:  python fixtures/rel/sbom/make_fixtures.py          (write)
      python fixtures/rel/sbom/make_fixtures.py --check  (verify, write nothing)
"""

import hashlib
import json
import re
import sys
import tomllib
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
REPO = "https://github.com/metrologymaven/metrologymaven"
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
RELEASE = "1.4.0"
COMMIT_TIME = "2026-11-17T09:30:00Z"
LINUX = "x86_64-unknown-linux-gnu"
WINDOWS = "x86_64-pc-windows-msvc"

OUT: dict[str, bytes] = {}


def put(rel: str, data):
    OUT[rel] = data.encode("utf-8") if isinstance(data, str) else data


def sha256(b: bytes) -> str:
    return hashlib.sha256(b).hexdigest()


def tool_version() -> str:
    cargo = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    return cargo["workspace"]["package"]["version"]


# ---------------------------------------------------------------- fixture workspace

DEP = {"name": "fid-sbom-fixture-dep", "version": "0.1.0", "license": "MIT OR Apache-2.0"}
DEP_FILES = {
    "Cargo.toml": (
        "[package]\n"
        f'name = "{DEP["name"]}"\n'
        f'version = "{DEP["version"]}"\n'
        'edition = "2021"\n'
        f'license = "{DEP["license"]}"\n'
        'description = "Stand-in third-party crate for the ECO.REL.SIGN.SBOM fixture"\n'
    ).encode(),
    "src/lib.rs": b"//! Stand-in third-party crate for the ECO.REL.SIGN.SBOM fixture.\n",
}
# Stand-in for the .crate archive digest: SHA-256 over "<path>\0<sha256>\n" of each file.
DEP_CHECKSUM = sha256("".join(f"{k}\0{sha256(v)}\n" for k, v in sorted(DEP_FILES.items())).encode())

# Workspace crates: name -> (licence, binary?, normal deps, deps only under cfg(windows)).
CRATES = {
    "fid-cli": ("MIT OR Apache-2.0", True, ["fid-core", DEP["name"]], ["fid-winio"]),
    "fid-core": ("MIT OR Apache-2.0", False, [], []),
    "fid-winio": ("Apache-2.0", False, [], []),
}


def crate_manifest(name, licence, deps, win_deps):
    lines = ["[package]", f'name = "{name}"', 'version = "0.1.0"', 'edition = "2021"',
             f'license = "{licence}"', "publish = false", ""]

    def dep_line(d):
        return f'{d} = "0.1"' if d == DEP["name"] else f'{d} = {{ path = "../{d}" }}'

    if deps:
        lines += ["[dependencies]"] + [dep_line(d) for d in deps] + [""]
    if win_deps:
        lines += ["[target.'cfg(windows)'.dependencies]"] + [dep_line(d) for d in win_deps] + [""]
    return "\n".join(lines)


def lockfile(packages):
    """packages: list of (name, version, source|None, checksum|None, deps)."""
    out = ["# This file is automatically @generated by Cargo.", "# It is not intended for manual editing.",
           "version = 4", ""]
    for name, version, source, checksum, deps in sorted(packages):
        out += ["[[package]]", f'name = "{name}"', f'version = "{version}"']
        if source:
            out.append(f'source = "{source}"')
        if checksum:
            out.append(f'checksum = "{checksum}"')
        if deps:
            out.append("dependencies = [")
            out += [f' "{d}",' for d in sorted(deps)]
            out.append("]")
        out.append("")
    return "\n".join(out)


def write_ws():
    put("ws/Cargo.toml", '[workspace]\nresolver = "2"\nmembers = ["fid-cli", "fid-core", "fid-winio"]\n')
    put("ws/.cargo/config.toml",
        "# Serve crates.io dependencies from ws/vendor/ so the fixture resolves offline.\n"
        '[source.crates-io]\nreplace-with = "vendored-sources"\n\n'
        '[source.vendored-sources]\ndirectory = "vendor"\n')
    for name, (licence, is_bin, deps, win) in CRATES.items():
        put(f"ws/{name}/Cargo.toml", crate_manifest(name, licence, deps, win))
        if is_bin:
            put(f"ws/{name}/src/main.rs", "//! Fixture binary for ECO.REL.SIGN.SBOM.\nfn main() {}\n")
        else:
            put(f"ws/{name}/src/lib.rs", "//! Fixture crate for ECO.REL.SIGN.SBOM.\n")
    vdir = f"ws/vendor/{DEP['name']}"
    for k, v in DEP_FILES.items():
        put(f"{vdir}/{k}", v)
    put(f"{vdir}/.cargo-checksum.json",
        json.dumps({"files": {k: sha256(v) for k, v in DEP_FILES.items()}, "package": DEP_CHECKSUM},
                   sort_keys=True, separators=(",", ":")))
    pkgs = [(n, "0.1.0", None, None, d + w) for n, (_, _, d, w) in CRATES.items()]
    pkgs.append((DEP["name"], DEP["version"], CRATES_IO, DEP_CHECKSUM, []))
    put("ws/Cargo.lock", lockfile(pkgs))

    # bad-licence: one binary crate with an invalid licence expression.
    put("bad-licence/ws/Cargo.toml",
        '[package]\nname = "bad-lic"\nversion = "0.1.0"\nedition = "2021"\nlicense = "GPL-2.0 OR"\n'
        'publish = false\n\n[workspace]\n')
    put("bad-licence/ws/src/main.rs", "//! Fixture binary with an invalid licence expression.\nfn main() {}\n")
    put("bad-licence/ws/Cargo.lock", lockfile([("bad-lic", "0.1.0", None, None, [])]))


def resolved(target):
    """Crates compiled into `fid-cli` for `target`, from the definitions above."""
    windows = target == WINDOWS
    seen, todo = set(), ["fid-cli"]
    while todo:
        n = todo.pop()
        if n in seen:
            continue
        seen.add(n)
        if n in CRATES:
            _, _, deps, win = CRATES[n]
            todo += deps + (win if windows else [])
    return seen


# ---------------------------------------------------------------- plan and native components

ARTEFACTS = [
    ("fid-cli-x86_64-linux.tar.gz", LINUX, b"fixture artefact: fid-cli for x86_64 Linux\n"),
    ("fid-cli-x86_64-windows.zip", WINDOWS, b"fixture artefact: fid-cli for x86_64 Windows\n"),
]
FONT = {
    "name": "Fixture Sans",
    "version": "1.0",
    "source": "https://example.org/fonts/fixture-sans-1.0.ttf",
    "license": "OFL-1.1",
    "sha256": sha256(b"Fixture Sans 1.0 stand-in font bytes\n"),
    "artefacts": ["fid"],
}


def toml_component(c):
    lines = ["[[component]]"]
    for k in ("name", "version", "source", "license", "sha256"):
        lines.append(f'{k} = "{c[k]}"')
    lines.append("artefacts = [" + ", ".join(f'"{a}"' for a in c["artefacts"]) + "]")
    return "\n".join(lines) + "\n"


def write_inputs():
    plan = {"version": RELEASE, "commit_time": COMMIT_TIME, "artefacts": []}
    for name, target, body in ARTEFACTS:
        put(f"artefacts/{name}", body)
        os_name = "windows" if target == WINDOWS else "linux"
        plan["artefacts"].append({"package": "fid", "kind": "cli", "os": os_name, "arch": "x86_64",
                                  "file_name": name, "target": target, "contents": ["fid-cli"],
                                  "features": [], "sha256": sha256(body)})
    put("plan.json", json.dumps(plan, indent=2) + "\n")
    header = "# Native (non-Rust) components bundled into artefacts: ECO.REL.SIGN.SBOM fixture.\n\n"
    put("native-components.toml", header + toml_component(FONT))
    put("native-bad-digest.toml", header + toml_component(dict(FONT, sha256="abc")))
    bad = {"version": RELEASE, "commit_time": COMMIT_TIME,
           "artefacts": [{"package": "bad", "file_name": "bad-lic-x86_64-linux.tar.gz", "target": LINUX,
                          "contents": ["bad-lic"], "sha256": sha256(b"bad")}]}
    put("bad-licence/plan.json", json.dumps(bad, indent=2) + "\n")
    put("bad-licence/native-components.toml", "# No native components.\n")


# ---------------------------------------------------------------- SPDX documents

UNRESERVED = set("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._~-")


def pct(s):
    return "".join(c if c in UNRESERVED else "".join(f"%{b:02X}" for b in c.encode()) for c in s)


def spdx_id_part(s):
    return re.sub(r"[^A-Za-z0-9.-]", "-", s)


def component(prefix, name, version, licence, purl, download, checksum):
    p = {"name": name, "versionInfo": version, "downloadLocation": download, "filesAnalyzed": False,
         "licenseConcluded": "NOASSERTION", "licenseDeclared": licence, "copyrightText": "NOASSERTION",
         "externalRefs": [{"referenceCategory": "PACKAGE-MANAGER", "referenceType": "purl",
                           "referenceLocator": purl}]}
    if checksum:
        p["checksums"] = [{"algorithm": "SHA256", "checksumValue": checksum}]
    return prefix, p


def document(artefact, target, digest, version):
    comps = []
    for n in resolved(target):
        if n in CRATES:
            licence, ver, dl, cs = CRATES[n][0], "0.1.0", "NOASSERTION", None
            qualifier = f"?repository_url={REPO}"
        else:
            licence, ver, cs = DEP["license"], DEP["version"], DEP_CHECKSUM
            dl = f"https://crates.io/api/v1/crates/{n}/{ver}/download"
            qualifier = ""
        comps.append(component("SPDXRef-Crate-", n, ver, licence, f"pkg:cargo/{pct(n)}@{pct(ver)}{qualifier}",
                               dl, cs))
    if "fid" in FONT["artefacts"]:
        f = FONT
        comps.append(component("SPDXRef-Native-", f["name"], f["version"], f["license"],
                               f"pkg:generic/{pct(f['name'])}@{pct(f['version'])}", f["source"], f["sha256"]))

    def key(c):
        p = c[1]
        return (p["name"], p["versionInfo"], p["externalRefs"][0]["referenceLocator"], p["downloadLocation"])

    comps.sort(key=key)
    used, packages = set(), []
    for prefix, p in comps:
        base = prefix + spdx_id_part(p["name"]) + "-" + spdx_id_part(p["versionInfo"])
        sid, i = base, 1
        while sid in used:
            i += 1
            sid = f"{base}-{i}"
        used.add(sid)
        p["SPDXID"] = sid
        packages.append(p)
    root = {"SPDXID": "SPDXRef-Artefact", "name": artefact, "versionInfo": RELEASE,
            "downloadLocation": "NOASSERTION", "filesAnalyzed": False, "licenseConcluded": "NOASSERTION",
            "licenseDeclared": "NOASSERTION", "copyrightText": "NOASSERTION",
            "checksums": [{"algorithm": "SHA256", "checksumValue": digest}]}
    rels = [{"spdxElementId": "SPDXRef-DOCUMENT", "relationshipType": "DESCRIBES",
             "relatedSpdxElement": "SPDXRef-Artefact"}]
    rels += [{"spdxElementId": "SPDXRef-Artefact", "relationshipType": "CONTAINS",
              "relatedSpdxElement": p["SPDXID"]} for p in packages]
    u = uuid.uuid5(uuid.NAMESPACE_URL, f"{REPO}/spdx/{RELEASE}/{artefact}")
    return {"spdxVersion": "SPDX-2.3", "dataLicense": "CC0-1.0", "SPDXID": "SPDXRef-DOCUMENT",
            "name": artefact, "documentNamespace": f"{REPO}/spdx/{artefact}-{u}",
            "creationInfo": {"created": COMMIT_TIME, "creators": [f"Tool: fid-xtask-{version}"]},
            "packages": [root] + packages, "relationships": rels}


def encode(doc):
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def write_expected():
    version = tool_version()
    for name, target, body in ARTEFACTS:
        put(f"expected/{name}.spdx.json", encode(document(name, target, sha256(body), version)))


# ---------------------------------------------------------------- structural SPDX 2.3 check

ID = re.compile(r"^SPDXRef-[A-Za-z0-9.-]+$")
HEX64 = re.compile(r"^[0-9a-f]{64}$")


def check_spdx(doc):
    """Required fields and value shapes of an SPDX 2.3 JSON document, from our reading of the
    specification (the full JSON schema is pinned by ECO.DATA.FETCH, not vendored here)."""
    errs = []
    for k in ("spdxVersion", "dataLicense", "SPDXID", "name", "documentNamespace", "creationInfo"):
        if k not in doc:
            errs.append(f"missing {k}")
    if doc.get("spdxVersion") != "SPDX-2.3" or doc.get("dataLicense") != "CC0-1.0":
        errs.append("bad spdxVersion or dataLicense")
    if doc.get("SPDXID") != "SPDXRef-DOCUMENT":
        errs.append("bad document SPDXID")
    if not re.match(r"^https://[^#\s]+$", doc.get("documentNamespace", "")):
        errs.append("bad documentNamespace")
    ci = doc.get("creationInfo", {})
    if not re.match(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$", ci.get("created", "")) or not ci.get("creators"):
        errs.append("bad creationInfo")
    ids = set()
    for p in doc.get("packages", []):
        for k in ("SPDXID", "name", "downloadLocation"):
            if k not in p:
                errs.append(f"package missing {k}")
        if not ID.match(p.get("SPDXID", "")) or p["SPDXID"] in ids:
            errs.append(f"bad or repeated SPDXID {p.get('SPDXID')}")
        ids.add(p.get("SPDXID"))
        if p.get("filesAnalyzed") is not False:
            errs.append(f"{p.get('SPDXID')}: filesAnalyzed must be false (no file list)")
        for c in p.get("checksums", []):
            if c.get("algorithm") != "SHA256" or not HEX64.match(c.get("checksumValue", "")):
                errs.append(f"{p.get('SPDXID')}: bad checksum")
        for r in p.get("externalRefs", []):
            if r.get("referenceCategory") != "PACKAGE-MANAGER" or r.get("referenceType") != "purl" \
                    or not r.get("referenceLocator", "").startswith("pkg:"):
                errs.append(f"{p.get('SPDXID')}: bad externalRef")
    for r in doc.get("relationships", []):
        if r.get("relationshipType") not in ("DESCRIBES", "CONTAINS", "BUILD_DEPENDENCY_OF"):
            errs.append("unexpected relationship type")
        for k in ("spdxElementId", "relatedSpdxElement"):
            if r.get(k) not in ids | {"SPDXRef-DOCUMENT"}:
                errs.append(f"relationship to unknown element {r.get(k)}")
    return errs


def main():
    check = "--check" in sys.argv[1:]
    write_ws()
    write_inputs()
    write_expected()
    bad = []
    for rel, data in sorted(OUT.items()):
        if rel.startswith("expected/"):
            errs = check_spdx(json.loads(data))
            if errs:
                bad.append(f"{rel}: {'; '.join(errs)}")
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(f"stale: {rel}")
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for b in bad:
        print(b)
    if bad:
        sys.exit(1)
    print("ok" if not check else "fixtures up to date")


if __name__ == "__main__":
    main()
