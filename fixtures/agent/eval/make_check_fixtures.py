#!/usr/bin/env python3
"""Generate the AGENT.EVAL.CHECK fixtures and their expected findings.

Independent of the Rust crate: the suites, job packages and catalog indexes are written
here with hashlib, json and tarfile, and the expected findings in check-expected.json are
computed by the small reference implementation of the check rules below (stdlib only),
never by running `fid-eval`.

Writes (next to this script):
  jobs/                          FLOW job packages (acceptance-job@1 subset) whose golden
                                 journals match suite-mini's golden digests
  catalog-index-mini.json        catalog index naming only suite-mini's workflow (AC-1)
  catalog-index-uncovered.json   the same plus a workflow no task names (AC-3)
  catalog-index-faults.json      catalog index for suite-faults (AC-5)
  suite-leak/                    a task granting a read root holding a copy of its oracle (AC-2)
  suite-prompt/                  a prompt containing "expected: CONFORMS" (AC-4)
  suite-faults/                  one suite with several different faults (AC-5)
  check-expected.json            per case: arguments, exit code and expected findings

suite-mini itself is written by make_fixtures.py (AGENT.EVAL.FORMAT) and only read here.
All data is synthetic and project-authored (CC-BY-4.0).
Run:  python fixtures/agent/eval/make_check_fixtures.py
"""

import hashlib
import io
import json
import math
import re
import shutil
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
JOBS = HERE / "jobs"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def write_bytes(p: Path, b: bytes) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(b)


def write_json(p: Path, obj) -> bytes:
    b = (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    write_bytes(p, b)
    return b


def start_archive(entries) -> bytes:
    """A deterministic ustar archive. entries: (name, bytes) for regular files or
    (name, ("symlink", target)) for a symbolic link. Names are written verbatim."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        for name, data in entries:
            ti = tarfile.TarInfo(name)
            ti.mtime = 0
            ti.mode = 0o644
            ti.uid = ti.gid = 0
            ti.uname = ti.gname = ""
            if isinstance(data, tuple):
                ti.type = tarfile.SYMTYPE
                ti.linkname = data[1]
                tf.addfile(ti)
            else:
                ti.size = len(data)
                tf.addfile(ti, io.BytesIO(data))
    return buf.getvalue()


# ---------------------------------------------------------------- job packages

GOLDEN = {
    "JOB.ACC.01": b"synthetic golden journal for JOB.ACC.01 (suite-mini)\n",
    "JOB.ACC.02": b"synthetic golden journal for JOB.ACC.02 (suite-mini)\n",
}
APPS = {"JOB.ACC.01": "inspect", "JOB.ACC.02": "probe"}

# The golden digests must be the ones suite-mini's tasks record (make_fixtures.py).
for tdir in sorted((HERE / "suite-mini" / "tasks").iterdir()):
    t = json.loads((tdir / "task.json").read_text(encoding="utf-8"))
    assert t["golden_digest"] == sha(GOLDEN[t["source_job"]]), t["id"]

if JOBS.exists():
    shutil.rmtree(JOBS)
for job_id, journal in GOLDEN.items():
    slug = "acc-" + job_id.rsplit(".", 1)[1]
    write_bytes(JOBS / slug / "golden-journal.txt", journal)
    write_json(JOBS / slug / "package.json", {
        "format": "acceptance-job@1",
        "job_id": job_id,
        "variant_id": slug + "-unassisted",
        "app": APPS[job_id],
        "milestones": [],
        "gating": "tracked",
        "assistance": "unassisted",
        "input_mode": "any",
        "journal": {"file": "golden-journal.txt", "digest": sha(journal)},
        "datasets": [],
        "checks": [{"id": "replay", "kind": "replay_outcome", "params": {}}],
        "time_targets": [{"metric": "task_s", "seconds": 600, "verified": False}],
    })


# ---------------------------------------------------------------- catalog indexes

def catalog(path: Path, workflows):
    rows = [{"catalog_version": sha(("catalog row " + w + "\n").encode("utf-8")), "workflow_id": w}
            for w in sorted(workflows)]
    write_json(path, {"manifest_digest": sha(b"synthetic manifest for AGENT.EVAL.CHECK\n"), "rows": rows})


catalog(HERE / "catalog-index-mini.json", ["inspect.evaluate_flatness"])
catalog(HERE / "catalog-index-uncovered.json", ["inspect.evaluate_flatness", "probe.measure_hole"])
catalog(HERE / "catalog-index-faults.json", ["inspect.evaluate_flatness", "report.export_pdf"])


# ---------------------------------------------------------------- suites

FLAT_ORACLE = {
    "items": [
        {"characteristic": "PL1-FLAT", "outcome": "CONFORMS", "basis": "POINTS",
         "value": {"amount": 1.25e-05, "unit": "m"}, "tol": {"amount": 1e-07, "unit": "m"}},
    ],
    "forbidden": [{"characteristic": "PL1-FLAT", "outcome": "NOT_DECIDABLE"}],
}
PART = b"<QIF synthetic placeholder 1/>\n"
PLAIN_PROMPT = "Evaluate the flatness of plane PL1 against its tolerance and report the decision."


def task_def(tid, start, prompt=PLAIN_PROMPT, source_job="JOB.ACC.01", golden=None, oracle=FLAT_ORACLE,
             roots=None, workflows=("inspect.evaluate_flatness",), licensed=None, given=None):
    return {
        "given": given,
        "id": tid, "start": start, "prompt": prompt, "source_job": source_job,
        "golden": golden if golden is not None else GOLDEN.get(source_job, b""),
        "oracle": oracle, "roots": roots or {"read": ["in"], "write": ["out"]},
        "workflows": list(workflows), "licensed": licensed,
    }


def build_suite(name, tasks):
    """tasks in dependency order; a start entry's data may be a function of the files
    built so far ({"oracle": {id: bytes}, "task": {id: bytes}, "archive": {id: bytes}})."""
    root = HERE / name
    if root.exists():
        shutil.rmtree(root)
    write_json(root / "suite.json", {"format": "fid-eval-suite@1", "name": name})
    built = {"oracle": {}, "task": {}, "archive": {}}
    for t in tasks:
        built["oracle"][t["id"]] = write_json(root / "oracles" / t["id"] / "oracle.json", t["oracle"])
    for t in tasks:
        entries = [(n, d(built) if callable(d) else d) for n, d in t["start"]]
        archive = start_archive(entries)
        write_bytes(root / "tasks" / t["id"] / "start.tar", archive)
        licensed = t["licensed"] or [n for n, d in t["start"]]
        task = {
            "id": t["id"],
            "source_job": t["source_job"],
            "golden_digest": sha(t["golden"]),
            "start_state": {"path": "start.tar", "digest": sha(archive)},
            "prompt": t["prompt"],
            "profile": "inspect-r1",
            "budget": {"wall_s": 600, "tool_calls": 40},
            "roots": t["roots"],
            "tier": "single_command",
            "apps": ["inspect"],
            "workflows": t["workflows"],
            "licences": [{"file": n, "spdx": "project-authored"} for n in sorted(licensed)],
        }
        if t["given"] is not None:
            task["given"] = t["given"]
        built["task"][t["id"]] = write_json(root / "tasks" / t["id"] / "task.json", task)
        built["archive"][t["id"]] = archive


# AC-2: the read root `in` holds a copy of the task's own oracle; the copy under `stash/`
# is outside every granted root and must not be reported.
build_suite("suite-leak", [
    task_def("leak-01", [
        ("in/part.qif", PART),
        ("in/notes.json", lambda b: b["oracle"]["leak-01"]),
        ("stash/oracle.json", lambda b: b["oracle"]["leak-01"]),
    ]),
])

# AC-4: the prompt gives away the decision for its own characteristic.
build_suite("suite-prompt", [
    task_def("prompt-01", [("in/part.qif", PART)],
             prompt="Evaluate PL1-FLAT against its tolerance and report the decision (expected: CONFORMS)."),
])

# AC-5: one suite, many different faults (with catalog-index-faults.json).
build_suite("suite-faults", [
    task_def("fa-base", [("in/part.qif", PART)]),
    # another task's file under the write root
    task_def("fa-leak", [("in/part.qif", PART), ("out/previous.json", lambda b: b["task"]["fa-base"])]),
    # a golden journal under the read root, under a harmless name
    task_def("fa-golden-copy", [("in/part.qif", PART), ("in/history.txt", GOLDEN["JOB.ACC.01"])]),
    # the oracle value, formatted in micrometres, in the prompt
    task_def("fa-value", [("in/part.qif", PART)],
             prompt="Plane PL1-FLAT was measured at 12.5 um; report the decision."),
    # golden digest that matches no package of the source job
    task_def("fa-golden", [("in/part.qif", PART)], golden=b"not the golden journal\n"),
    # source job without a package
    task_def("fa-nojob", [("in/part.qif", PART)], source_job="JOB.ACC.99", golden=b"journal of a missing job\n"),
    # unsafe archive entries: parent segment, absolute path, symbolic link
    task_def("fa-dotdot", [("in/part.qif", PART), ("../escape.txt", b"x\n")], licensed=["in/part.qif"]),
    task_def("fa-abs", [("in/part.qif", PART), ("/abs.txt", b"x\n")], licensed=["in/part.qif"]),
    task_def("fa-link", [("in/part.qif", PART), ("in/oracle", ("symlink", "../../oracles/fa-link/oracle.json"))],
             licensed=["in/part.qif"]),
])


# Hand-labelled cases the reference shares its design with, so their expectations are
# written by hand below (HAND), not computed: duplicate and case-colliding archive paths,
# a root reached through case or a trailing dot, a lower-case outcome in the prompt, and
# the `given` exclusion.
H_PROMPT_GIVEN = "Plane PL1-FLAT has a drawing nominal of 12.5 um; evaluate it and report the decision."
build_suite("suite-hostile", [
    task_def("h-dup", [("in/part.qif", PART), ("in/notes.json", b"harmless notes\n"),
                       ("in/notes.json", lambda b: b["oracle"]["h-dup"])],
             licensed=["in/part.qif", "in/notes.json"]),
    task_def("h-case", [("in/part.qif", PART), ("IN/oracle.json", lambda b: b["oracle"]["h-case"])]),
    task_def("h-dot", [("in/part.qif", PART), ("in./copy.txt", GOLDEN["JOB.ACC.01"])]),
    task_def("h-collide", [("in/part.qif", PART), ("in/Part.qif", b"another part\n")]),
    task_def("h-lower", [("in/part.qif", PART)],
             prompt="Check pl1-flat against its tolerance; expected: conforms."),
    task_def("h-given", [("in/part.qif", PART)], prompt=H_PROMPT_GIVEN,
             given=[{"amount": 1.25e-05, "unit": "m"}]),
    task_def("h-given-other", [("in/part.qif", PART)], prompt=H_PROMPT_GIVEN,
             given=[{"amount": 1e-05, "unit": "m"}]),
])
HAND = sorted([
    ("parse_error", "h-dup", "tasks/h-dup/start.tar"),
    ("oracle_leak", "h-case", "IN/oracle.json"),
    ("oracle_leak", "h-dot", "in./copy.txt"),
    ("parse_error", "h-collide", "tasks/h-collide/start.tar"),
    ("oracle_in_prompt", "h-lower", "tasks/h-lower/task.json"),
    ("oracle_in_prompt", "h-given-other", "tasks/h-given-other/task.json"),
])


# ---------------------------------------------------------------- reference check

def norm_entry(name):
    """Normalised archive path, or None if the entry is unsafe."""
    if name.startswith("/") or "\\" in name:
        return None
    segs = [s for s in name.split("/") if s not in ("", ".")]
    if not segs or any(s == ".." or ":" in s or fold_seg(s) == "" for s in segs):
        return None
    return "/".join(segs)


def fold_seg(s):
    """ASCII-lowercase, trailing dots and spaces removed (Windows / macOS resolution)."""
    return "".join(c.lower() if c.isascii() else c for c in s.rstrip(". "))


def fold(path):
    return "/".join(fold_seg(s) for s in path.split("/"))


def unpack(archive: bytes):
    """[(path, folded, sha)] of regular files, or None if any entry is unsafe, not a
    file/dir, or shares its folded path with another entry."""
    out = []
    seen = set()
    with tarfile.open(fileobj=io.BytesIO(archive), mode="r:") as tf:
        for m in tf.getmembers():
            if not (m.isfile() or m.isdir()):
                return None
            p = norm_entry(m.name)
            if p is None or fold(p) in seen:
                return None
            seen.add(fold(p))
            if m.isfile():
                f = tf.extractfile(m)
                out.append((p, fold(p), sha(f.read())))
    return out


UNIT_SCALES = {
    "m": [("m", 1.0), ("mm", 1e-3), ("cm", 1e-2), ("um", 1e-6), ("µm", 1e-6), ("μm", 1e-6),
          ("nm", 1e-9)],
    "m2": [("m2", 1.0), ("mm2", 1e-6)],
    "m3": [("m3", 1.0), ("mm3", 1e-9)],
    "rad": [("rad", 1.0), ("mrad", 1e-3), ("deg", math.pi / 180.0), ("°", math.pi / 180.0)],
}
NUMBER = re.compile(r"(?<![A-Za-z0-9_.+\-])([+-]?(?:\d+(?:\.\d+)?|\.\d+)(?:[eE][+-]?\d+)?)", re.ASCII)


def has_token(text, tok):
    """tok occurs with no ASCII letter, digit, `_` or `-` directly before or after it."""
    pat = r"(?<![A-Za-z0-9_\-])" + re.escape(tok) + r"(?![A-Za-z0-9_\-])"
    return re.search(pat, text, re.ASCII | re.IGNORECASE) is not None


def value_in_prompt(prompt, amount, unit, tol, given=()):
    scales = UNIT_SCALES.get(unit, [(unit, 1.0)] if unit != "1" else [])
    for m in NUMBER.finditer(prompt):
        n = float(m.group(1))
        rest = prompt[m.end():].lstrip(" \t")
        for sym, scale in scales:
            if rest.startswith(sym) and not re.match(r"[A-Za-z0-9_]", rest[len(sym):len(sym) + 1]):
                si = n * scale
                if any(g["unit"] == unit and abs(si - g["amount"]) <= tol for g in given):
                    continue  # a value the task deliberately gives the agent
                if abs(si - amount) <= tol:
                    return True
    return False


def reference_check(suite_dir: Path, catalog_path: Path, jobs_dir: Path):
    findings = []
    tasks = {}
    for tdir in sorted((suite_dir / "tasks").iterdir()):
        tid = tdir.name
        tb = (tdir / "task.json").read_bytes()
        t = json.loads(tb)
        ob = (suite_dir / "oracles" / tid / "oracle.json").read_bytes()
        ab = (tdir / t["start_state"]["path"]).read_bytes()
        assert sha(ab) == t["start_state"]["digest"]  # the loader's rule; not exercised here
        tasks[tid] = {"task": t, "task_bytes": tb, "oracle": json.loads(ob), "oracle_bytes": ob, "archive": ab}

    # job packages
    golden_by_job = {}
    golden_digests = set()
    for pdir in sorted(jobs_dir.iterdir()):
        pkg = json.loads((pdir / "package.json").read_text(encoding="utf-8"))
        declared = pkg["journal"]["digest"]
        actual = sha((pdir / pkg["journal"]["file"]).read_bytes())
        assert declared == actual
        golden_by_job.setdefault(pkg["job_id"], set()).add(declared)
        golden_digests |= {declared, actual}

    for tid, x in tasks.items():
        t = x["task"]
        tfile = f"tasks/{tid}/task.json"
        if t["source_job"] not in golden_by_job:
            findings.append({"rule": "unknown_job", "task": tid, "file": tfile, "subject": t["source_job"]})
        elif t["golden_digest"] not in golden_by_job[t["source_job"]]:
            findings.append({"rule": "digest_mismatch", "task": tid, "file": tfile, "subject": "golden_digest"})

    # coverage
    cat = json.loads(catalog_path.read_text(encoding="utf-8"))
    covered = {w for x in tasks.values() for w in x["task"]["workflows"]}
    for row in cat["rows"]:
        if row["workflow_id"] not in covered:
            findings.append({"rule": "uncovered_workflow", "task": None, "file": None,
                             "subject": row["workflow_id"]})

    # isolation
    oracle_digests = {sha(x["oracle_bytes"]) for x in tasks.values()}
    for tid, x in tasks.items():
        entries = unpack(x["archive"])
        if entries is None:
            findings.append({"rule": "parse_error", "task": tid, "file": f"tasks/{tid}/start.tar",
                             "subject": None})
            continue
        others = set()
        for oid, y in tasks.items():
            if oid != tid:
                others |= {sha(y["task_bytes"]), sha(y["archive"])}
        roots = [fold(r) for r in x["task"]["roots"]["read"] + x["task"]["roots"]["write"]]
        for p, fp, d in entries:
            if not any(fp == r or fp.startswith(r + "/") for r in roots):
                continue
            if d in oracle_digests or d in golden_digests or d in others:
                findings.append({"rule": "oracle_leak", "task": tid, "file": p, "subject": None})

    # prompts
    for tid, x in tasks.items():
        prompt = x["task"]["prompt"]
        for item in x["oracle"]["items"]:
            c = item["characteristic"]
            if has_token(prompt, c) and has_token(prompt, item["outcome"]):
                findings.append({"rule": "oracle_in_prompt", "task": tid, "file": f"tasks/{tid}/task.json",
                                 "subject": c})
            if "value" in item:
                tol = item.get("tol", {"amount": 1e-9})["amount"]
                if value_in_prompt(prompt, item["value"]["amount"], item["value"]["unit"], tol,
                                   x["task"].get("given", [])):
                    findings.append({"rule": "oracle_in_prompt", "task": tid,
                                     "file": f"tasks/{tid}/task.json", "subject": c})

    findings.sort(key=lambda f: (f["task"] or "", f["rule"], f["file"] or "", f["subject"] or ""))
    return findings


CASES = [
    ("ac1-mini", "suite-mini", "catalog-index-mini.json"),
    ("ac2-leak", "suite-leak", "catalog-index-mini.json"),
    ("ac3-uncovered", "suite-mini", "catalog-index-uncovered.json"),
    ("ac4-prompt", "suite-prompt", "catalog-index-mini.json"),
    ("ac5-faults", "suite-faults", "catalog-index-faults.json"),
    ("hand-hostile", "suite-hostile", "catalog-index-mini.json"),
]

out = []
for name, suite, cat in CASES:
    f = reference_check(HERE / suite, HERE / cat, JOBS)
    out.append({"name": name, "suite": suite, "catalog": cat, "jobs": "jobs", "exit": 1 if f else 0,
                "findings": f})

# Hand-labelled expectations (the spec's acceptance criteria) guard the reference itself.
by = {c["name"]: c for c in out}
assert by["ac1-mini"]["findings"] == []
assert [(f["rule"], f["task"], f["file"]) for f in by["ac2-leak"]["findings"]] == \
    [("oracle_leak", "leak-01", "in/notes.json")]
assert [(f["rule"], f["subject"]) for f in by["ac3-uncovered"]["findings"]] == \
    [("uncovered_workflow", "probe.measure_hole")]
assert [(f["rule"], f["task"]) for f in by["ac4-prompt"]["findings"]] == [("oracle_in_prompt", "prompt-01")]
assert sorted((f["rule"], f["task"] or "", f["file"] or "") for f in by["ac5-faults"]["findings"]) == sorted([
    ("oracle_leak", "fa-leak", "out/previous.json"),
    ("oracle_leak", "fa-golden-copy", "in/history.txt"),
    ("oracle_in_prompt", "fa-value", "tasks/fa-value/task.json"),
    ("digest_mismatch", "fa-golden", "tasks/fa-golden/task.json"),
    ("unknown_job", "fa-nojob", "tasks/fa-nojob/task.json"),
    ("parse_error", "fa-dotdot", "tasks/fa-dotdot/start.tar"),
    ("parse_error", "fa-abs", "tasks/fa-abs/start.tar"),
    ("parse_error", "fa-link", "tasks/fa-link/start.tar"),
    ("uncovered_workflow", "", ""),
])

assert sorted((f["rule"], f["task"], f["file"]) for f in by["hand-hostile"]["findings"]) == HAND, \
    by["hand-hostile"]["findings"]

write_json(HERE / "check-expected.json", {"cases": out})
print(f"{len(out)} check cases written")
