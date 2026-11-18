#!/usr/bin/env python3
"""Generate the AGENT.EVAL.FORMAT fixtures (independent of the Rust crate).

Writes:
  suite-mini/            a 2-task suite in the AGENT.EVAL.FORMAT layout (AC-1, AC-2)
  expected.json          the golden suite version, computed here with hashlib and
                         RFC 8785 rules (REQ-AGENT.EVAL.FORMAT-005)
  cases/*.json           single task / oracle files, valid and malformed
  cases.json             hand-labelled expectation per case file (AC-3, AC-4, REQ-004/006)

Expected outcomes in cases.json are stated by hand from the spec's rules, never by
running the Rust parser. All data is synthetic and project-authored (CC-BY-4.0).
Run:  python fixtures/agent/eval/make_fixtures.py
"""

import hashlib
import io
import json
import shutil
import tarfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SUITE = HERE / "suite-mini"
CASES = HERE / "cases"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def write_bytes(p: Path, b: bytes) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(b)


def write_json(p: Path, obj) -> bytes:
    b = (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")
    write_bytes(p, b)
    return b


def start_archive(files: dict) -> bytes:
    """A deterministic ustar archive (mtime 0, uid/gid 0, mode 0644)."""
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as tf:
        for name in sorted(files):
            data = files[name].encode("utf-8")
            ti = tarfile.TarInfo(name)
            ti.size, ti.mtime, ti.mode, ti.uid, ti.gid, ti.uname, ti.gname = len(data), 0, 0o644, 0, 0, "", ""
            tf.addfile(ti, io.BytesIO(data))
    return buf.getvalue()


def jcs_string_pairs(pairs) -> bytes:
    """RFC 8785 for a list of [ascii string, ascii string] pairs: no whitespace,
    no escapes needed (all characters are printable ASCII without quote or backslash)."""
    for a, b in pairs:
        for s in (a, b):
            assert all(0x20 <= ord(c) < 0x7F and c not in '"\\' for c in s), s
    return json.dumps(pairs, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


# ---------------------------------------------------------------- suite-mini

if SUITE.exists():
    shutil.rmtree(SUITE)
if CASES.exists():
    shutil.rmtree(CASES)

write_json(SUITE / "suite.json", {"format": "fid-eval-suite@1", "name": "suite-mini"})

TASKS = [
    {
        "id": "mini-inspect-01",
        "source_job": "JOB.ACC.01",
        "golden_text": "synthetic golden journal for JOB.ACC.01 (suite-mini)\n",
        "start": {"in/part.qif": "<QIF synthetic placeholder 1/>\n", "README.txt": "suite-mini task 1\n"},
        "prompt": "Evaluate the flatness of plane PL1 against its tolerance and report the decision.",
        "profile": "inspect-r1",
        "budget": {"wall_s": 600, "tool_calls": 40},
        "roots": {"read": ["in"], "write": ["out"]},
        "tier": "single_command",
        "apps": ["inspect"],
        "workflows": ["inspect.evaluate_flatness"],
        "oracle": {
            "items": [
                {"characteristic": "PL1-FLAT", "outcome": "CONFORMS", "basis": "POINTS",
                 "value": {"amount": 1.25e-05, "unit": "m"}, "tol": {"amount": 1e-07, "unit": "m"}},
            ],
            "forbidden": [{"characteristic": "PL1-FLAT", "outcome": "NOT_DECIDABLE"}],
        },
    },
    {
        "id": "mini-probe-01",
        "source_job": "JOB.ACC.02",
        "golden_text": "synthetic golden journal for JOB.ACC.02 (suite-mini)\n",
        "start": {"in/program.txt": "synthetic probe program\n"},
        "prompt": "Run the probing program, then report hole H1 diameter and position decisions.",
        "profile": "probe-r1",
        "budget": {"wall_s": 1800, "tool_calls": 120},
        "roots": {"read": ["in", "ref/data"], "write": ["out/report"]},
        "tier": "multi_step",
        "apps": ["probe", "report"],
        "workflows": [],
        "oracle": {
            "items": [
                {"characteristic": "H1-DIA", "outcome": "CONFORMS", "basis": "FEATURES",
                 "value": {"amount": 0.01, "unit": "m"}},
                {"characteristic": "H1-POS", "outcome": "NONCONFORMS", "basis": "FEATURES"},
            ],
            "forbidden": [],
        },
    },
]

pairs = []
for t in TASKS:
    tid = t["id"]
    archive = start_archive(t["start"])
    write_bytes(SUITE / "tasks" / tid / "start.tar", archive)
    task = {
        "id": tid,
        "source_job": t["source_job"],
        "golden_digest": sha(t["golden_text"].encode("utf-8")),
        "start_state": {"path": "start.tar", "digest": sha(archive)},
        "prompt": t["prompt"],
        "profile": t["profile"],
        "budget": t["budget"],
        "roots": t["roots"],
        "tier": t["tier"],
        "apps": t["apps"],
        "workflows": t["workflows"],
        "licences": [{"file": name, "spdx": "project-authored"} for name in sorted(t["start"])],
    }
    tb = write_json(SUITE / "tasks" / tid / "task.json", task)
    ob = write_json(SUITE / "oracles" / tid / "oracle.json", t["oracle"])
    pairs += [
        (f"tasks/{tid}/task.json", sha(tb)),
        (f"tasks/{tid}/start.tar", sha(archive)),
        (f"oracles/{tid}/oracle.json", sha(ob)),
    ]

pairs.sort(key=lambda p: p[0])
version = sha(jcs_string_pairs([list(p) for p in pairs]))
write_json(HERE / "expected.json", {
    "suite": "suite-mini",
    "suite_version": version,
    "tasks": len(TASKS),
    "files": [list(p) for p in pairs],
})

# ---------------------------------------------------------------- cases

VALID_TASK = {
    "id": "case-task",
    "source_job": "JOB.ACC.03",
    "golden_digest": sha(b"synthetic golden journal for cases\n"),
    "start_state": {"path": "start.tar", "digest": sha(b"synthetic start state for cases\n")},
    "prompt": "Synthetic prompt.",
    "profile": "inspect-r1",
    "budget": {"wall_s": 60, "tool_calls": 5},
    "roots": {"read": ["in"], "write": ["out"]},
    "tier": "full_job",
    "apps": ["inspect", "network"],
    "workflows": ["inspect.report_all"],
    "licences": [{"file": "in/part.qif", "spdx": "CC-BY-4.0"}],
}

ITEM = {"characteristic": "C1", "outcome": "CONFORMS", "basis": "POINTS",
        "value": {"amount": 0.002, "unit": "m"}, "tol": {"amount": 1e-06, "unit": "m"}}

cases = []


def task_case(name, mutate, code, fields, schema):
    t = json.loads(json.dumps(VALID_TASK))
    mutate(t)
    write_json(CASES / name, t)
    cases.append({"file": name, "kind": "task", "code": code, "fields": fields, "schema": schema})


def oracle_case(name, oracle, code, fields, schema, expect_tols=None):
    write_json(CASES / name, oracle)
    c = {"file": name, "kind": "oracle", "code": code, "fields": fields, "schema": schema}
    if expect_tols is not None:
        c["tols"] = expect_tols
    cases.append(c)


def raw_case(name, kind, text, code, fields, schema):
    write_bytes(CASES / name, text.encode("utf-8"))
    cases.append({"file": name, "kind": kind, "code": code, "fields": fields, "schema": schema})


def setk(path, value):
    def m(t):
        o = t
        for k in path[:-1]:
            o = o[k]
        o[path[-1]] = value
    return m


def delk(key):
    return lambda t: t.pop(key)


# Valid baselines.
task_case("task-valid.json", lambda t: None, None, [], "valid")
# AC-3: tier "easy".
task_case("task-bad-tier.json", setk(["tier"], "easy"), "invalid_task", ["tier"], "invalid")
# REQ-004: app tags.
task_case("task-empty-apps.json", setk(["apps"], []), "invalid_task", ["apps"], "invalid")
task_case("task-bad-app.json", setk(["apps"], ["inspect", "cmm"]), "invalid_task", ["apps[1]"], "invalid")
task_case("task-dup-app.json", setk(["apps"], ["probe", "probe"]), "invalid_task", ["apps[1]"], "invalid")
# Errors are collected, not first-only.
task_case("task-multi-error.json", lambda t: (t.update(tier="easy", apps=[]), t["budget"].update(wall_s=0)),
          "invalid_task", ["apps", "budget.wall_s", "tier"], "invalid")
# REQ-001 fields.
task_case("task-missing-prompt.json", delk("prompt"), "invalid_task", ["prompt"], "invalid")
task_case("task-empty-prompt.json", setk(["prompt"], ""), "invalid_task", ["prompt"], "invalid")
task_case("task-bad-golden.json", setk(["golden_digest"], "sha256:" + "A" * 64), "invalid_task",
          ["golden_digest"], "invalid")
task_case("task-bad-start-digest.json", setk(["start_state", "digest"], "md5:00"), "invalid_task",
          ["start_state.digest"], "invalid")
task_case("task-zero-budget.json", setk(["budget", "tool_calls"], 0), "invalid_task", ["budget.tool_calls"],
          "invalid")
task_case("task-fraction-budget.json", setk(["budget", "wall_s"], 1.5), "invalid_task", ["budget.wall_s"],
          "invalid")
task_case("task-abs-root.json", setk(["roots", "read"], ["/etc"]), "invalid_task", ["roots.read[0]"], "invalid")
task_case("task-dotdot-root.json", setk(["roots", "write"], ["out", "../oracles"]), "invalid_task",
          ["roots.write[1]"], "invalid")
task_case("task-backslash-root.json", setk(["roots", "read"], ["in\\sub"]), "invalid_task", ["roots.read[0]"],
          "invalid")
task_case("task-abs-start.json", setk(["start_state", "path"], "C:/x/start.tar"), "invalid_task",
          ["start_state.path"], "invalid")
task_case("task-bad-id.json", setk(["id"], "../escape"), "invalid_task", ["id"], "invalid")
task_case("task-workflow-not-string.json", setk(["workflows"], [7]), "invalid_task", ["workflows[0]"], "invalid")
task_case("task-unknown-field.json", setk(["oracle_hint"], "C1 conforms"), "invalid_task", ["oracle_hint"],
          "invalid")
# REQ-AGENT.EVAL-006: licence for every input file.
task_case("task-no-licences.json", setk(["licences"], []), "invalid_task", ["licences"], "invalid")
task_case("task-bad-spdx.json", setk(["licences"], [{"file": "in/part.qif", "spdx": "GPL v3"}]), "invalid_task",
          ["licences[0].spdx"], "invalid")
task_case("task-dup-licence.json", setk(["licences"], [{"file": "a.qif", "spdx": "MIT"},
                                                       {"file": "a.qif", "spdx": "CC0-1.0"}]),
          "invalid_task", ["licences[1].file"], "valid")

# REQ-006: duplicate key, nesting depth (structural; not a schema question).
dup = json.dumps(VALID_TASK, indent=2)
dup = dup.replace('"tier": "full_job"', '"tier": "full_job",\n  "tier": "multi_step"')
raw_case("task-dup-key.json", "task", dup + "\n", "duplicate_key", ["tier"], "n/a")


def nested(n):
    """n nested arrays, innermost empty: depth n."""
    return "[" * n + "]" * n


# Top-level object is depth 1 and "workflows" holds the arrays: 1 + 64 = 65 > 64.
deep = json.dumps(dict(VALID_TASK, workflows="@@"), indent=2).replace('"@@"', nested(64))
raw_case("task-deep-65.json", "task", deep + "\n", "input_limit", [], "n/a")
# 1 + 63 = 64: within the limit, so it parses and then fails validation.
ok_depth = json.dumps(dict(VALID_TASK, workflows="@@"), indent=2).replace('"@@"', "[" + nested(62) + "]")
raw_case("task-depth-64.json", "task", ok_depth + "\n", "invalid_task", ["workflows[0]"], "invalid")
raw_case("task-not-json.json", "task", '{"id": "x", "tier": }\n', "invalid_task", [], "n/a")

# Oracles. AC-4: value without tol -> tol 1e-9 in the value's unit.
oracle_case("oracle-valid.json", {"items": [ITEM], "forbidden": [{"characteristic": "C1",
                                                                  "outcome": "NONCONFORMS"}]},
            None, [], "valid", expect_tols=[{"amount": 1e-06, "unit": "m"}])
oracle_case("oracle-no-tol.json", {"items": [
    {"characteristic": "C1", "outcome": "CONFORMS", "basis": "POINTS", "value": {"amount": 0.002, "unit": "m"}},
    {"characteristic": "C2", "outcome": "NONCONFORMS", "basis": "FEATURES",
     "value": {"amount": "0x3f50624dd2f1a9fc", "unit": "rad"}},
    {"characteristic": "C3", "outcome": "INDETERMINATE", "basis": "RESULTS_ONLY"},
], "forbidden": []}, None, [], "valid",
    expect_tols=[{"amount": 1e-09, "unit": "m"}, {"amount": 1e-09, "unit": "rad"}, None])
it = dict(ITEM)
del it["value"]
oracle_case("oracle-tol-only.json", {"items": [it], "forbidden": []}, "invalid_oracle", ["items[0].tol"],
            "invalid")
oracle_case("oracle-unit-mismatch.json", {"items": [dict(ITEM, tol={"amount": 1e-06, "unit": "rad"})],
                                          "forbidden": []}, "invalid_oracle", ["items[0].tol.unit"], "valid")
oracle_case("oracle-non-si-unit.json", {"items": [dict(ITEM, value={"amount": 2.0, "unit": "mm"}, tol=None)],
                                        "forbidden": []}, "invalid_oracle",
            ["items[0].value.unit"], "invalid")
oracle_case("oracle-negative-tol.json", {"items": [dict(ITEM, tol={"amount": -1e-06, "unit": "m"})],
                                         "forbidden": []}, "invalid_oracle", ["items[0].tol.amount"], "invalid")
oracle_case("oracle-bad-bits.json", {"items": [dict(ITEM, value={"amount": "0x7ff0000000000000", "unit": "m"})],
                                     "forbidden": []}, "invalid_oracle", ["items[0].value.amount"], "valid")
oracle_case("oracle-bad-outcome.json", {"items": [dict(ITEM, outcome="PASS")], "forbidden": []},
            "invalid_oracle", ["items[0].outcome"], "invalid")
oracle_case("oracle-bad-basis.json", {"items": [dict(ITEM, basis="CAD")], "forbidden": []},
            "invalid_oracle", ["items[0].basis"], "invalid")
oracle_case("oracle-empty.json", {"items": [], "forbidden": []}, "invalid_oracle", ["items"], "invalid")
oracle_case("oracle-dup-characteristic.json", {"items": [ITEM, dict(ITEM, outcome="NONCONFORMS")],
                                               "forbidden": []}, "invalid_oracle", ["items[1].characteristic"],
            "valid")
oracle_case("oracle-contradiction.json", {"items": [ITEM], "forbidden": [{"characteristic": "C1",
                                                                         "outcome": "CONFORMS"}]},
            "invalid_oracle", ["forbidden[0]"], "valid")
oracle_case("oracle-bad-forbidden.json", {"items": [ITEM], "forbidden": [{"characteristic": "C1",
                                                                         "outcome": "FAIL"}]},
            "invalid_oracle", ["forbidden[0].outcome"], "invalid")

# Clean None-valued keys (used above to drop optional fields) and rewrite those files.
for c in cases:
    p = CASES / c["file"]
    if c["kind"] == "oracle":
        o = json.loads(p.read_text(encoding="utf-8"))
        for item in o.get("items", []):
            for k in [k for k, v in item.items() if v is None]:
                del item[k]
        write_json(p, o)

write_json(HERE / "cases.json", cases)
print(f"suite-mini version {version}; {len(cases)} cases")
