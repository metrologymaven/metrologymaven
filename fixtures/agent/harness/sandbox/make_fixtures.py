#!/usr/bin/env python3
"""Generate the AGENT.HARNESS.SANDBOX fixtures (independent of the Rust crate).

Writes:
  cases/<name>/task.json   a task file (copied from suite-mini/mini-inspect-01 with the
  cases/<name>/start.tar   start-state digest and roots changed) and its start-state archive
  expected.json            for the two suite-mini tasks: the start-state digest, the unpacked
                           files (path, SHA-256) read back with Python's tarfile, the granted
                           read-root files and the root lists; for every case: the expected
                           error code

Digests are computed here with hashlib; expected error codes are hand-labelled from the
spec's rules (REQ-AGENT.HARNESS.SANDBOX-002, Design "Errors") and the PR's stated
fail-closed rules, never by running the Rust code. All data is synthetic and project-authored.

Run:          python fixtures/agent/harness/sandbox/make_fixtures.py
Check only:   python fixtures/agent/harness/sandbox/make_fixtures.py --check
"""

import hashlib
import io
import json
import sys
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
SUITE = REPO / "fixtures" / "agent" / "eval" / "suite-mini"
CASES = HERE / "cases"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def info(name: str, size: int = 0, kind=tarfile.REGTYPE, linkname: str = "") -> tarfile.TarInfo:
    ti = tarfile.TarInfo(name)
    ti.size, ti.mtime, ti.uid, ti.gid, ti.uname, ti.gname = size, 0, 0, 0, "", ""
    ti.type = kind
    ti.mode = 0o755 if kind == tarfile.DIRTYPE else 0o644
    ti.linkname = linkname
    return ti


def archive(entries, fmt=tarfile.USTAR_FORMAT, pax=None) -> bytes:
    """entries: list of (name, bytes | None for a directory, optional (type, linkname))."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=fmt, pax_headers=pax or {}) as tf:
        for e in entries:
            name, data = e[0], e[1]
            if len(e) > 2:
                kind, link = e[2]
                tf.addfile(info(name, 0, kind, link))
            elif data is None:
                tf.addfile(info(name, 0, tarfile.DIRTYPE))
            else:
                tf.addfile(info(name, len(data)), io.BytesIO(data))
    return buf.getvalue()


def base_files():
    return [("README.txt", b"case\n"), ("in/part.qif", b"<QIF case/>\n")]


def make_cases():
    """name -> (archive bytes, read roots, write roots, expected code, recorded digest or None).

    Code `ok` means the build must succeed; the expected files are listed in expected.json."""
    ok = archive(base_files())
    original = (SUITE / "tasks" / "mini-inspect-01" / "start.tar").read_bytes()
    # One altered byte inside the content of README.txt (header 512 bytes, data next).
    altered = bytearray(original)
    altered[512] ^= 0x01
    bad_sum = bytearray(ok)
    bad_sum[148:156] = b"0000000\x00"
    cases = {
        # Positive case with type-5 directory entries (one empty) and a nested file.
        "ok": (archive([("in", None), ("in/sub", None), ("in/empty", None), ("README.txt", b"ok\n"),
                        ("in/sub/a.txt", b"a\n")]), ["in"], ["out"], "ok", None),
        # AC-2: the task names the original digest; the archive has one byte changed.
        "altered": (bytes(altered), ["in"], ["out"], "start_state_mismatch", sha(original)),
        "dotdot": (archive([("in/../../evil.txt", b"x\n")]), ["in"], ["out"], "bad_archive", None),
        "parent": (archive([("../evil.txt", b"x\n")]), ["in"], ["out"], "bad_archive", None),
        "absolute": (archive([("/abs.txt", b"x\n")]), ["in"], ["out"], "bad_archive", None),
        "drive": (archive([("C:/x.txt", b"x\n")]), ["in"], ["out"], "bad_archive", None),
        "backslash": (archive([("in\\x.txt", b"x\n")]), ["in"], ["out"], "bad_archive", None),
        "symlink": (archive([("in/link", None, (tarfile.SYMTYPE, "../../outside"))]), ["in"], ["out"],
                    "bad_archive", None),
        "hardlink": (archive([("in/a.txt", b"x\n"), ("in/b.txt", None, (tarfile.LNKTYPE, "in/a.txt"))]),
                     ["in"], ["out"], "bad_archive", None),
        "pax": (archive(base_files(), fmt=tarfile.PAX_FORMAT, pax={"comment": "x"}), ["in"], ["out"],
                "bad_archive", None),
        "case-collision": (archive([("in/A.txt", b"a\n"), ("in/a.txt", b"b\n")]), ["in"], ["out"],
                           "bad_archive", None),
        "case-dir-file": (archive([("In", b"a\n"), ("in/x.txt", b"b\n")]), ["in"], ["out"], "bad_archive",
                          None),
        "duplicate": (archive([("in/a.txt", b"a\n"), ("in/a.txt", b"b\n")]), ["in"], ["out"], "bad_archive",
                      None),
        "file-then-dir": (archive([("in", b"a\n"), ("in/x.txt", b"b\n")]), ["in"], ["out"], "bad_archive",
                          None),
        "reserved-name": (archive([("in/CON.txt", b"a\n")]), ["in"], ["out"], "bad_archive", None),
        "trailing-dot": (archive([("in/a.", b"a\n")]), ["in"], ["out"], "bad_archive", None),
        "bad-checksum": (bytes(bad_sum), ["in"], ["out"], "bad_archive", None),
        "truncated": (ok[:512 + 3], ["in"], ["out"], "bad_archive", None),
        "trailing-garbage": (ok + b"\x01" + b"\x00" * 511, ["in"], ["out"], "bad_archive", None),
        "roots-case": (ok, ["in", "IN"], ["out"], "invalid_roots", None),
        "roots-nested": (ok, ["in", "in/sub"], ["out"], "invalid_roots", None),
        "roots-dotdot": (ok, [".."], ["out"], "invalid_roots", None),
        "roots-absolute": (ok, ["/etc"], ["out"], "invalid_roots", None),
        "roots-reserved": (ok, ["in"], ["aux"], "invalid_roots", None),
        "roots-file": (ok, ["README.txt"], ["out"], "invalid_roots", None),
    }
    return cases


# Hand-labelled part of each failure's detail, from the rule that refuses it (see the PR).
REASONS = {
    "altered": "task names",
    "dotdot": "`.` or `..` segment",
    "parent": "`.` or `..` segment",
    "absolute": "empty segment",
    "drive": "character outside",
    "backslash": "character outside",
    "symlink": "is not a regular file or directory",
    "hardlink": "is not a regular file or directory",
    "pax": "is not a regular file or directory",
    "case-collision": "collides with another entry",
    "case-dir-file": "lies below a file",
    "duplicate": "collides with another entry",
    "file-then-dir": "lies below a file",
    "reserved-name": "Windows device name",
    "trailing-dot": "segment ends with a dot",
    "bad-checksum": "header checksum mismatch",
    "truncated": "truncated data",
    "trailing-garbage": "data after the end-of-archive block",
    "roots-case": "equal or nested",
    "roots-nested": "equal or nested",
    "roots-dotdot": "`.` or `..` segment",
    "roots-absolute": "empty segment",
    "roots-reserved": "Windows device name",
    "roots-file": "is a file in the start state",
}


def task_json(template: dict, digest: str, read, write) -> bytes:
    t = json.loads(json.dumps(template))
    t["start_state"]["digest"] = digest
    t["roots"] = {"read": read, "write": write}
    return (json.dumps(t, indent=2) + "\n").encode("utf-8")


def unpacked(tar_bytes: bytes):
    files = []
    with tarfile.open(fileobj=io.BytesIO(tar_bytes), mode="r:") as tf:
        for m in tf.getmembers():
            if m.isfile():
                f = tf.extractfile(m)
                files.append([m.name, sha(f.read())])
    return sorted(files)


def build():
    out = {}
    template = json.loads((SUITE / "tasks" / "mini-inspect-01" / "task.json").read_text("utf-8"))
    tasks = {}
    for tid in ("mini-inspect-01", "mini-probe-01"):
        tdir = SUITE / "tasks" / tid
        task = json.loads((tdir / "task.json").read_text("utf-8"))
        data = (tdir / task["start_state"]["path"]).read_bytes()
        files = unpacked(data)
        read = task["roots"]["read"]
        tasks[tid] = {
            "start_digest": sha(data),
            "task_digest": task["start_state"]["digest"],
            "files": files,
            "read_root_files": sorted(
                [["roots/read/" + p, d] for p, d in files if any(p.startswith(r + "/") for r in read)]),
            "read": read,
            "write": task["roots"]["write"],
        }
    out["tasks"] = tasks
    case_files = {}
    codes = {}
    reasons = {}
    for name, (data, read, write, code, digest) in sorted(make_cases().items()):
        case_files[f"cases/{name}/start.tar"] = data
        case_files[f"cases/{name}/task.json"] = task_json(template, digest or sha(data), read, write)
        codes[name] = code
        if code == "ok":
            out["ok_files"] = unpacked(data)
        else:
            reasons[name] = REASONS[name]
    out["cases"] = codes
    out["reasons"] = reasons
    case_files["expected.json"] = (json.dumps(out, indent=2, sort_keys=True) + "\n").encode("utf-8")
    return case_files


def main():
    files = build()
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, data in sorted(files.items()):
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != data:
                stale.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale))
        sys.exit(1)
    print(("checked " if check else "wrote ") + f"{len(files)} files")


if __name__ == "__main__":
    main()
