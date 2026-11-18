"""Fixtures for spec node OUTR.SITE.GEN.IMPORT (generated-input lock file, fetch, digest check).

Independent reference generator (stdlib only): it writes the synthetic site folders, the remote
release served by the fetch test, and every expected output, computing digests and the RFC 8785
build manifest here in Python, never by running the Rust code. Fixture content is CC-BY-4.0.

    python fixtures/outr/site/import/make_fixtures.py          # (re)write the fixtures
    python fixtures/outr/site/import/make_fixtures.py --check  # fail if any file differs

Layout:
    ok/              two releases (v0.1.0, v0.1.1), each a release-manifest.json and four reports,
                     plus an app list, a token file and a spec report; all pinned in inputs.lock.toml
    tampered/        ok with one byte of inputs/v0.1.0/parity.json changed (lock unchanged)
    wrong-release/   ok with inputs/v0.1.0/status.json changed and its lock entry updated to match,
                     so only the release manifest disagrees
    stray/           ok plus an unlisted inputs/notes.txt
    remote/v0.2.0/   one release as a release host serves it (manifest and four reports)
    remote-bad/v0.2.0/  the same release with parity.md altered after the manifest was written
    fetch/hosts.toml host allowlist for the fetch tests (127.0.0.1, no example.org)
    expected/        build-manifest.json for ok (revision abc123, date 2026-11-17), the finding
                     lines of the three failing fixtures, and the lock file after fetching v0.2.0
"""

import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

LOCK_HEADER = (
    "# Generated-input lock file (spec node OUTR.SITE.GEN.IMPORT); entries sorted by path.\n"
    "# Written by `fid-outreach site import fetch`; review every change in its pull request.\n"
)
FIELDS = ["name", "kind", "release", "source_rev", "date", "path", "sha256"]
REPORTS = ["parity.json", "parity.md", "status.json", "status.md"]


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canon(obj):
    # RFC 8785 for this data: only ASCII strings, arrays and objects (no numbers).
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def lock_text(entries):
    out = LOCK_HEADER
    for e in sorted(entries, key=lambda e: e["path"]):
        out += "\n[[input]]\n"
        for k in FIELDS:
            if k in e:
                out += f'{k} = "{e[k]}"\n'
    return out.encode("utf-8")


def reports(version, date):
    return {
        "parity.json": (json.dumps({"release": version, "date": date, "rows": [
            {"feature": "probe-calibration", "state": "partial"},
            {"feature": "plane-fit", "state": "done"},
        ]}, indent=2, sort_keys=True) + "\n").encode(),
        "parity.md": f"# Parity report {version}\n\n| Feature | State |\n|---|---|\n"
                     f"| plane-fit | done |\n| probe-calibration | partial |\n".encode(),
        "status.json": (json.dumps({"release": version, "apps": [{"id": "fid-studio", "status": "alpha"}]},
                                   indent=2, sort_keys=True) + "\n").encode(),
        "status.md": f"# Status {version}\n\nfid-studio: alpha\n".encode(),
    }


def manifest(version, date, commit, files, bare_hex):
    arts = [{
        "name": f"fid-{version}-x86_64-linux.tar.gz", "kind": "archive", "os": "linux", "arch": "x86_64",
        "tier": "1", "url": f"https://releases.example.invalid/v{version}/fid-{version}-x86_64-linux.tar.gz",
        "size": "1048576", "sha256": "sha256:" + "ab" * 32,
    }]
    for name, data in files.items():
        d = hashlib.sha256(data).hexdigest()
        arts.append({"name": name, "kind": "report", "os": "any", "arch": "any", "tier": "1",
                     "url": f"https://releases.example.invalid/v{version}/{name}", "size": str(len(data)),
                     "sha256": d if bare_hex else "sha256:" + d})
    arts.sort(key=lambda a: a["name"])
    m = {"version": version, "channel": "beta", "commit": commit, "date": date,
         "apps": [{"id": "fid-studio", "status": "alpha"}], "venues": ["release-host"], "artefacts": arts}
    return (json.dumps(m, indent=2, sort_keys=True) + "\n").encode()


def release(tag, date, commit, bare_hex=False):
    files = reports(tag[1:], date)
    files_m = dict(files)
    m = manifest(tag[1:], date, commit, files, bare_hex)
    out = {f"inputs/{tag}/release-manifest.json": m}
    for n, d in files_m.items():
        out[f"inputs/{tag}/{n}"] = d
    entries = [{"name": "release-manifest.json", "kind": "release_manifest", "release": tag, "date": date,
                "path": f"inputs/{tag}/release-manifest.json", "sha256": sha(m)}]
    for n, d in files_m.items():
        entries.append({"name": n, "kind": "release_asset", "release": tag, "date": date,
                        "path": f"inputs/{tag}/{n}", "sha256": sha(d)})
    return out, entries


def ok_site():
    files, entries = {}, []
    for tag, date, commit, bare in [("v0.1.0", "2026-10-01", "1" * 40, False),
                                    ("v0.1.1", "2026-10-20", "2" * 40, True)]:
        f, e = release(tag, date, commit, bare)
        files.update(f)
        entries += e
    apps = b'[[app]]\nid = "fid-studio"\nname = "Studio"\nstatus = "alpha"\n'
    tokens = (json.dumps({"color": {"ink": "#1a1a1a", "paper": "#ffffff"}}, indent=2, sort_keys=True) + "\n").encode()
    coverage = b"# COVERAGE\n\n| Domain | Leaves | Built |\n|---|---|---|\n| OUTR | 40 | 12 |\n"
    files["inputs/apps/apps.toml"] = apps
    files["inputs/tokens/tokens.json"] = tokens
    files["inputs/v0.1.1/COVERAGE.md"] = coverage
    entries += [
        {"name": "apps.toml", "kind": "app_list", "source_rev": "3" * 40, "date": "2026-11-02",
         "path": "inputs/apps/apps.toml", "sha256": sha(apps)},
        {"name": "tokens.json", "kind": "tokens", "source_rev": "4" * 40, "date": "2026-11-03",
         "path": "inputs/tokens/tokens.json", "sha256": sha(tokens)},
        {"name": "COVERAGE.md", "kind": "spec_report", "release": "v0.1.1", "source_rev": "2" * 40,
         "date": "2026-10-20", "path": "inputs/v0.1.1/COVERAGE.md", "sha256": sha(coverage)},
    ]
    return files, entries


def build():
    out = {}
    files, entries = ok_site()

    def site(prefix, fs, es):
        for p, d in fs.items():
            out[f"{prefix}/{p}"] = d
        out[f"{prefix}/inputs.lock.toml"] = lock_text(es)

    site("ok", files, entries)

    # tampered: one byte of v0.1.0/parity.json changed, lock unchanged.
    t = dict(files)
    orig = t["inputs/v0.1.0/parity.json"]
    t["inputs/v0.1.0/parity.json"] = orig.replace(b"partial", b"partiaL", 1)
    site("tampered", t, entries)
    lock_parity = next(e for e in entries if e["path"] == "inputs/v0.1.0/parity.json")["sha256"]
    out["expected/tampered.out"] = (
        f"IMPORT-DIGEST parity.json {lock_parity} {sha(t['inputs/v0.1.0/parity.json'])}\n").encode()

    # wrong-release: status.json changed and its lock entry follows; the manifest does not.
    w = dict(files)
    w["inputs/v0.1.0/status.json"] = w["inputs/v0.1.0/status.json"].replace(b"alpha", b"beta")
    we = [dict(e) for e in entries]
    for e in we:
        if e["path"] == "inputs/v0.1.0/status.json":
            e["sha256"] = sha(w["inputs/v0.1.0/status.json"])
    site("wrong-release", w, we)
    out["expected/wrong-release.out"] = b"IMPORT-RELEASE status.json v0.1.0\n"

    # stray: an unlisted file.
    s = dict(files)
    s["inputs/notes.txt"] = b"scratch notes, not an input\n"
    site("stray", s, entries)
    out["expected/stray.out"] = b"IMPORT-UNLISTED inputs/notes.txt\n"

    # build manifest of ok.
    bm = {"site_revision": "abc123", "revision_date": "2026-11-17", "inputs": [
        {k: e[k] for k in FIELDS if k in e} for e in sorted(entries, key=lambda e: e["path"])]}
    out["expected/build-manifest.json"] = canon(bm)

    # remote release v0.2.0 and the lock after fetching it into a copy of ok.
    rfiles, rentries = release("v0.2.0", "2026-11-10", "5" * 40)
    for p, d in rfiles.items():
        out["remote/" + p[len("inputs/"):]] = d
    bad = dict(rfiles)
    bad["inputs/v0.2.0/parity.md"] = bad["inputs/v0.2.0/parity.md"].replace(b"done", b"DONE")
    for p, d in bad.items():
        out["remote-bad/" + p[len("inputs/"):]] = d
    out["expected/fetch-lock.toml"] = lock_text(entries + rentries)

    out["fetch/hosts.toml"] = (
        b"# Project-controlled host allowlist (OUTR.SITE.GEN.IMPORT); test copy for the fetch tests.\n\n"
        b'[[host]]\nname = "127.0.0.1"\npurpose = "site"\n\n'
        b'[[host]]\nname = "Releases.MetrologyMaven.example"\npurpose = "release_host"\n')
    return out


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    bad = []
    for rel, data in sorted(files.items()):
        path = os.path.join(HERE, *rel.split("/"))
        if check:
            try:
                with open(path, "rb") as f:
                    if f.read() != data:
                        bad.append(rel)
            except OSError:
                bad.append(rel)
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(data)
    if check:
        for d in ["ok", "tampered", "wrong-release", "stray", "remote", "remote-bad", "fetch", "expected"]:
            for root, _, names in os.walk(os.path.join(HERE, d)):
                for n in names:
                    rel = os.path.relpath(os.path.join(root, n), HERE).replace(os.sep, "/")
                    if rel not in files:
                        bad.append(rel + " (not generated)")
        if bad:
            print("stale fixtures:\n  " + "\n  ".join(bad))
            return 1
        print(f"{len(files)} fixture files up to date")
    return 0


if __name__ == "__main__":
    sys.exit(main())
