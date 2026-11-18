"""Writes the acceptance-job package fixtures for FLOW.RUNNER.PKG and their digests.

Each package folder is the `valid/` package with one deliberate change. This script is the
independent reference for the digests: it computes the journal SHA-256 with hashlib and the
package digest as SHA-256 of the RFC 8785 (JCS) form of the manifest, written here with the
standard library only (no floats in the fixtures, ASCII keys, so `json.dumps` with sorted
keys and no whitespace is the JCS form). The expected findings in `expected.txt` are
labelled by hand from the spec's rules (REQ-FLOW.RUNNER.PKG-001..006), never by running the
Rust validator.

Run from the repository root:  python fixtures/flow/pkg/make_fixtures.py [--check]
With --check, nothing is written; the script fails if a committed file differs.
Standard library only.
"""

import copy
import hashlib
import json
import pathlib
import sys

OUT = pathlib.Path(__file__).resolve().parent
CHECK = "--check" in sys.argv[1:]
PROBLEMS = []

# A 3-entry synthetic golden journal (JSON Lines; the validator only digests and counts it).
JOURNAL = (
    '{"seq":"1","command":"import.cad_step@1","principal":"human"}\n'
    '{"seq":"2","command":"align.run_bestfit@1","principal":"human"}\n'
    '{"seq":"3","command":"report.export_pdf@1","principal":"human"}\n'
).encode("ascii")
TRUTH = b'{"format":"truth@0","values":{"c1":"10.000"}}\n'
DATASET_BYTES = b"flow-pkg demo part\n"
DATASET_SHA = hashlib.sha256(DATASET_BYTES).hexdigest()


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def no_floats(v):
    if isinstance(v, float):
        raise ValueError("fixtures must not hold floats (JCS number form not reproduced here)")
    if isinstance(v, dict):
        for k, x in v.items():
            if not k.isascii():
                raise ValueError("non-ASCII key")
            no_floats(x)
    if isinstance(v, list):
        for x in v:
            no_floats(x)


def jcs(v):
    no_floats(v)
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def emit(rel, data):
    path = OUT / rel
    if CHECK:
        if not path.exists() or path.read_bytes() != data:
            PROBLEMS.append(rel)
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


def pretty(doc):
    return (json.dumps(doc, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


BASE = {
    "format": "acceptance-job@1",
    "job_id": "JOB.ACC.01",
    "variant_id": "demo-unassisted",
    "app": "inspect",
    "milestones": ["launch-1"],
    "gating": "gating",
    "assistance": "unassisted",
    "input_mode": "any",
    "journal": {"file": "journal.jsonl", "sha256": sha(JOURNAL)},
    "datasets": [{"id": "flow-pkg-demo-part", "sha256": "sha256:" + DATASET_SHA}],
    "checks": [
        {"id": "replay", "kind": "replay_outcome", "params": {}},
        {"id": "values", "kind": "value_within_u", "params": {"truth": "truth.json"}},
    ],
    "time_targets": [
        {"metric": "task_s", "seconds": 1200, "verified": False},
        {"metric": "machine_s", "seconds": 60, "verified": False},
    ],
}

KILL_CHECK = {"id": "survive", "kind": "committed_survive_kill", "params": {}}


def package(name, change=None, raw=None):
    doc = copy.deepcopy(BASE)
    if change:
        change(doc)
    emit(f"{name}/package.json", raw if raw is not None else pretty(doc))
    emit(f"{name}/journal.jsonl", JOURNAL)
    emit(f"{name}/truth.json", TRUTH)
    return doc


def drop(key):
    return lambda d: d.pop(key)


valid = package("valid")
emit("valid.digest", (sha(jcs(valid)) + "\n").encode("ascii"))
tracked = copy.deepcopy(valid)
tracked["gating"] = "tracked"
emit("valid-tracked.digest", (sha(jcs(tracked)) + "\n").encode("ascii"))

# AC-2..AC-4
package("no-gating", drop("gating"))
package("bad-check", lambda d: d["checks"][1].update(kind="value_close"))
package("kill-no-check", lambda d: d.update(kill_after=2))

# Further rules and hostile inputs.
def kill_out_of_range(d):
    d["kill_after"] = 4
    d["checks"].append(KILL_CHECK)


package("kill-out-of-range", kill_out_of_range)
package("survive-no-kill", lambda d: d["checks"].append(KILL_CHECK))
package("gating-no-task", lambda d: d.update(time_targets=[d["time_targets"][1]]))
package("bad-journal-digest", lambda d: d["journal"].update(sha256=sha(b"not the journal\n")))
package("unknown-dataset", lambda d: d["datasets"][0].update(id="flow-pkg-missing"))
package("dataset-digest", lambda d: d["datasets"][0].update(sha256=sha(b"other data\n")))
package("traversal", lambda d: d["journal"].update(file="../valid/journal.jsonl"))
package("absolute-param", lambda d: d["checks"][1]["params"].update(truth="/etc/passwd"))
package("dup-check-id", lambda d: d["checks"][1].update(id="replay"))
package("case-collision", lambda d: d["checks"][1]["params"].update(truth="Journal.jsonl"))
package("wrong-case", lambda d: d["journal"].update(file="JOURNAL.jsonl"))
package("extra-field", lambda d: d.update(gating_override="tracked"))
package("bad-params", lambda d: d["checks"][1].update(params={"tolerance": 1}))
package("missing-journal", lambda d: d["journal"].update(file="missing.jsonl"))
package("no-checks", lambda d: d.update(checks=[]))
package("device-name", lambda d: d["checks"][1]["params"].update(truth="con.json"))
package("names-manifest", lambda d: d["checks"][1]["params"].update(truth="PACKAGE.json"))

# Duplicate key: written as raw text, since a dict cannot hold one.
dup_text = pretty(BASE).decode("utf-8").replace(
    '  "gating": "gating",\n', '  "gating": "gating",\n  "gating": "tracked",\n'
)
assert dup_text.count('"gating": ') == 2
package("dup-key", raw=dup_text.encode("utf-8"))

# Dataset catalogue (ECO.DATA.CAT form) with the one dataset the packages name.
emit(
    "catalogue.toml",
    (
        "# Dataset catalogue for the FLOW.RUNNER.PKG fixtures (ECO.DATA.CAT form).\n"
        "# Only `id` and `sha256` are read by the package validator.\n\n"
        '[policy]\nredistributable = ["CC-BY-4.0"]\n\n'
        "[[dataset]]\n"
        'id = "flow-pkg-demo-part"\n'
        'title = "Synthetic demo part for the package fixtures"\n'
        'source = "https://github.com/metrologymaven/metrologymaven"\n'
        'licence = "CC-BY-4.0"\n'
        f"size_bytes = {len(DATASET_BYTES)}\n"
        f'sha256 = "{DATASET_SHA}"\n'
        'owner = "FLOW"\n'
        'redistribution = "mirror"\n'
        'urls = ["https://github.com/metrologymaven/metrologymaven"]\n'
    ).encode("ascii"),
)

# Suite indexes, relative to this folder.
emit(
    "suite.json",
    pretty({"format": "acceptance-suite@1", "packages": [{"path": "valid", "digest": sha(jcs(valid))}]}),
)
emit(
    "suite-stale.json",
    pretty({"format": "acceptance-suite@1", "packages": [{"path": "valid", "digest": sha(jcs(tracked))}]}),
)
d = sha(jcs(valid))
emit(
    "suite-bad.json",
    pretty(
        {
            "format": "acceptance-suite@1",
            "packages": [
                {"path": "valid", "digest": d},
                {"path": "valid", "digest": d},
                {"path": "VALID", "digest": d},
                {"path": "valid/inner", "digest": d},
                {"path": "../valid", "digest": d},
                {"path": "bad-check", "digest": "sha256:XYZ"},
            ],
        }
    ),
)

if CHECK:
    if PROBLEMS:
        print("stale fixtures:", ", ".join(PROBLEMS))
        sys.exit(1)
    print("fixtures/flow/pkg up to date")
