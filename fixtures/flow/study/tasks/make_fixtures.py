"""Writes the study task list fixtures for spec node FLOW.STUDY.PROTO.

Each fixture is the shipped task list (``acceptance/study/tasks.json``) with one change.
The expected findings are NOT computed here: they are labelled by hand from the spec's
rules in ``expected.txt``. Stdlib only. Run from anywhere:

    python fixtures/flow/study/tasks/make_fixtures.py          # rewrite the fixtures
    python fixtures/flow/study/tasks/make_fixtures.py --check  # fail if any is stale
"""

import copy
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent

BASE = {
    "format": "study-tasks@1",
    "record_format": "study-record@1",
    "tasks": [
        {
            "job_id": "JOB.ACC.01",
            "start_command": "import.cad_step@1",
            "end_command": "report.export_pdf@1",
            "target_ref": "scan-gdt-report",
            "maps": ["per-02"],
            "personas": ["JOB.PER.02"],
        },
        {
            "job_id": "JOB.ACC.02",
            "start_command": "import.results_dmis@1",
            "end_command": "report.export_pdf@1",
            "target_ref": "fai-as9102-report",
            "maps": ["per-01"],
            "personas": ["JOB.PER.01"],
        },
        {
            "job_id": "JOB.ACC.03",
            "start_command": "import.results_csv@1",
            "end_command": "report.export_pdf@1",
            "target_ref": "fai-drawing-only-report",
            "maps": ["per-01"],
            "personas": ["JOB.PER.01"],
        },
    ],
}


def variant(change):
    doc = copy.deepcopy(BASE)
    change(doc)
    return doc


def task(doc, job):
    return next(t for t in doc["tasks"] if t["job_id"] == job)


def add_acc99(d):
    # AC-2: a job no journey map exercises.
    d["tasks"].append(
        {
            "job_id": "JOB.ACC.99",
            "start_command": "import.results_dmis@1",
            "end_command": "report.export_pdf@1",
            "target_ref": "fai-as9102-report",
            "maps": ["per-01"],
            "personas": ["JOB.PER.01"],
        }
    )


FIXTURES = {
    "valid.json": variant(lambda d: None),
    "no-map.json": variant(add_acc99),
    "bad-command.json": variant(lambda d: task(d, "JOB.ACC.02").update(start_command="import.results_dmis")),
    "unknown-persona.json": variant(lambda d: task(d, "JOB.ACC.01").update(personas=["JOB.PER.42"])),
    "unknown-map.json": variant(lambda d: task(d, "JOB.ACC.03").update(maps=["per-99"])),
    "wrong-order.json": variant(
        lambda d: task(d, "JOB.ACC.02").update(
            start_command="report.export_pdf@1", end_command="import.results_dmis@1"
        )
    ),
    "extra-field.json": variant(lambda d: task(d, "JOB.ACC.02").update(part_name="bracket")),
    "missing-j3.json": variant(lambda d: d.update(tasks=d["tasks"][:2])),
    "wrong-format.json": variant(lambda d: d.update(format="study-tasks@2")),
}


def render(doc):
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, doc in FIXTURES.items():
        path = HERE / name
        text = render(doc)
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(name)
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
