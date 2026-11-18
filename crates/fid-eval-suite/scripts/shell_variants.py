#!/usr/bin/env python3
"""Shell-variant generator of the red-team suite (spec node AGENT.REDTEAM.SHELL,
REQ-AGENT.REDTEAM.SHELL-001). Python standard library only.

Copies every task of the base suites (the injection, escalation, self-adoption and
fabrication families) into a shell variant that differs only in requiring an adapter with
shell access:

  id          shell-<base id>
  family      shell (a variant carries no `subfamily`)
  variant_of  <base id>
  shell       true
  markers     every FIDRT- marker in the task file, the start-state archive (file names and
              contents) and the oracle is replaced by a fresh marker derived from the variant ID
              and the old marker (SHA-256, 26 Crockford base32 characters), so a committed
              entry carrying the base task's marker never trips the variant's predicate
  start state rebuilt as a deterministic ustar archive (sorted names, mtime 0, uid/gid 0,
              mode 0644); `start_state.digest` updated

Everything else (prompt, profile, budget, roots, tier, apps, workflows, licences, payload
vectors, predicates, oracle) is copied unchanged. `fid-eval suite check --suite <shell>
--base <dir>...` verifies the result (exactly one variant per base task, equal except for the
shell and fresh markers).

Library use (fixtures/agent/redteam/shell/make_shell.py): variants(root, base_rels).
Command line: python crates/fid-eval-suite/scripts/shell_variants.py --out <shell suite dir>
              <base suite dir>...   (writes tasks/ and oracles/ of the variants only)
"""

import hashlib
import io
import json
import re
import sys
import tarfile
from pathlib import Path

CROCKFORD = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
MARKER = re.compile(rb"FIDRT-[0-9A-HJKMNP-TV-Z]{26}")
PREFIX = "shell-"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def jbytes(obj) -> bytes:
    return (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def fresh_marker(variant_id: str, old: str) -> str:
    """FIDRT- plus 26 Crockford base32 characters from SHA-256 of the variant ID and old marker."""
    n = int.from_bytes(hashlib.sha256(f"fid-redteam-shell {variant_id} {old}".encode("utf-8")).digest(), "big")
    return "FIDRT-" + "".join(CROCKFORD[(n >> (5 * i)) & 31] for i in range(26))


def read_archive(data: bytes) -> dict:
    """{name: bytes} of the regular files of a ustar archive."""
    out = {}
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:") as tf:
        for m in tf.getmembers():
            if not m.isfile():
                raise ValueError(f"start state holds a non-regular member: {m.name}")
            f = tf.extractfile(m)
            out[m.name] = f.read() if f else b""
    return out


def write_archive(files: dict) -> bytes:
    """A deterministic ustar archive of {name: bytes}."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        for name in sorted(files):
            data = files[name]
            ti = tarfile.TarInfo(name)
            ti.size, ti.mtime, ti.mode, ti.uid, ti.gid, ti.uname, ti.gname = len(data), 0, 0o644, 0, 0, "", ""
            tf.addfile(ti, io.BytesIO(data))
    return buf.getvalue()


def make_variant(task_bytes: bytes, start_bytes: bytes, oracle_bytes: bytes):
    """(variant task dict, start archive bytes, oracle bytes, {old marker: new marker})."""
    base = json.loads(task_bytes)
    for k in ("variant_of", "shell_only", "shell"):
        if k in base:
            raise ValueError(f"{base['id']}: a base task must not carry {k}")
    if base.get("family") in (None, "shell"):
        raise ValueError(f"{base['id']}: a base task must be a red-team task of a non-shell family")
    vid = PREFIX + base["id"]
    files = read_archive(start_bytes)
    found = set(MARKER.findall(task_bytes)) | set(MARKER.findall(oracle_bytes))
    for name, data in files.items():
        found |= set(MARKER.findall(name.encode("utf-8"))) | set(MARKER.findall(data))
    mapping = {m.decode("ascii"): fresh_marker(vid, m.decode("ascii")) for m in sorted(found)}

    def sub(b: bytes) -> bytes:
        for old, new in mapping.items():
            b = b.replace(old.encode("ascii"), new.encode("ascii"))
        return b

    start = write_archive({sub(n.encode("utf-8")).decode("utf-8"): sub(d) for n, d in files.items()})
    replaced = json.loads(sub(task_bytes))
    doc = {}
    for k, v in replaced.items():
        if k == "id":
            doc[k] = vid
        elif k == "subfamily":
            continue
        elif k == "family":
            doc["family"] = "shell"
            doc["variant_of"] = base["id"]
            doc["shell"] = True
        elif k == "start_state":
            doc[k] = {"path": v["path"], "digest": sha(start)}
        else:
            doc[k] = v
    return doc, start, sub(oracle_bytes), mapping


def variants(root: Path, base_rels):
    """Every variant of the base suites, in base order then task-ID order:
    [{"id", "variant_of", "base_suite", "task", "start", "oracle", "markers"}]."""
    out = []
    for rel in base_rels:
        sdir = root / rel
        for tdir in sorted(p for p in (sdir / "tasks").iterdir() if p.is_dir()):
            tb = (tdir / "task.json").read_bytes()
            spath = json.loads(tb)["start_state"]["path"]
            ob = (sdir / "oracles" / tdir.name / "oracle.json").read_bytes()
            doc, start, oracle, mapping = make_variant(tb, (tdir / spath).read_bytes(), ob)
            out.append({"id": doc["id"], "variant_of": tdir.name, "base_suite": rel, "task": doc, "start": start,
                        "oracle": oracle, "markers": mapping})
    ids = [v["id"] for v in out]
    if len(set(ids)) != len(ids):
        raise ValueError("variant IDs repeat (base task IDs repeat across the base suites)")
    return out


def files_of(v: dict, suite_rel: str) -> dict:
    """{repo-relative path: bytes} of one variant inside the shell suite."""
    spath = v["task"]["start_state"]["path"]
    return {f"{suite_rel}/tasks/{v['id']}/task.json": jbytes(v["task"]),
            f"{suite_rel}/tasks/{v['id']}/{spath}": v["start"],
            f"{suite_rel}/oracles/{v['id']}/oracle.json": v["oracle"]}


def main(argv) -> int:
    if len(argv) < 3 or argv[0] != "--out":
        print(__doc__)
        return 2
    out_dir, bases = Path(argv[1]), [Path(b) for b in argv[2:]]
    for v in variants(Path("."), bases):
        for rel, b in files_of(v, str(out_dir)).items():
            p = Path(rel)
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b)
        print("wrote", v["id"])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
