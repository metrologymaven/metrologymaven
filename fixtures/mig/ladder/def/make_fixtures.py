"""Writes the MIG.LADDER.DEF fixtures and the reference digest of ladder-ok.json.

Independent of the Rust code (brief rule: expected values never come from the code under
test). The ladder files are fictional test data built from one base dictionary; the variants
change exactly one thing each, as the acceptance criteria describe:

- ladder-ok.json      stages 1 to 3 commands `built`, stages 4 and 5 commands `planned`
- forward.json        stage 2 (templates) lists `incumbent_point_export` as a prerequisite,
                      an input only of stage 3 (re_evaluate)                      -> AC-2
- basis.json          stage 4 (history) declares POINTS                           -> AC-5
- manifest-ok.json    a fictional registry manifest extract with every `built` command and
                      a few demo commands, without the planned stage 4 and 5 ones -> AC-1, AC-4
- manifest-missing.json  manifest-ok.json without migrate.import_form@1           -> AC-3
- ladder-ok.digest    `sha256:` + hex SHA-256 of the RFC 8785 canonical JSON of ladder-ok.json,
                      computed here with the stdlib (sorted keys, no whitespace; the fixtures
                      hold only ASCII strings and small integers, for which this equals JCS).

Usage: python make_fixtures.py [--check]   (--check compares bytes and writes nothing)
Licence of the generated fixtures: CC0-1.0.
"""

import copy
import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def cmd(command_id, spec_node, availability):
    return {"command_id": command_id, "spec_node": spec_node, "availability": availability}


BASE = {
    "schema": "mig-ladder@1",
    "version": "0.9.0",
    "stages": [
        {
            "id": "coexist",
            "order": 1,
            "phase": "P1a",
            "inputs": ["incumbent_results_file"],
            "outputs": ["imported_results", "transcription_record"],
            "commands": [
                cmd("import.results_dmis@1", "IO.RES.TRANS.DMIS", "built"),
                cmd("import.results_csv@1", "IO.RES.CSV.IN", "built"),
            ],
            "basis": ["RESULTS_ONLY"],
            "prerequisites": [],
            "completion_evidence": ["import.results_dmis@1", "import.results_csv@1"],
        },
        {
            "id": "templates",
            "order": 2,
            "phase": "P1a",
            "inputs": ["incumbent_report_form"],
            "outputs": ["form_map", "filled_report"],
            "commands": [
                cmd("migrate.import_form@1", "MIG.TEMPL.MAP", "built"),
                cmd("migrate.fill_form@1", "MIG.TEMPL.FILL", "built"),
            ],
            "basis": ["RESULTS_ONLY"],
            "prerequisites": [{"input": "imported_results"}],
            "completion_evidence": ["migrate.fill_form@1"],
        },
        {
            "id": "re_evaluate",
            "order": 3,
            "phase": "P1a",
            "inputs": ["incumbent_point_export"],
            "outputs": ["reevaluated_results"],
            "commands": [
                cmd("migrate.import_point_export@1", "MIG.PROJ.POINTS", "built"),
                cmd("correlate.run_parallel@1", "MIG.PARALLEL.REPORT", "built"),
            ],
            "basis": ["POINTS", "FEATURES"],
            "prerequisites": [{"command": "import.results_csv@1"}],
            "completion_evidence": ["migrate.import_point_export@1"],
        },
        {
            "id": "history",
            "order": 4,
            "phase": "P1b",
            "inputs": ["incumbent_results_archive"],
            "outputs": ["historical_results"],
            "commands": [cmd("stats.import_history@1", "RPT.CATALOG.IMPORT.BATCH", "planned")],
            "basis": ["RESULTS_ONLY"],
            "prerequisites": [],
            "completion_evidence": ["stats.import_history@1"],
        },
        {
            "id": "programs",
            "order": 5,
            "phase": "P2",
            "inputs": ["incumbent_program"],
            "outputs": ["equivalence_report"],
            "commands": [cmd("migrate.prove_equivalence@1", "MIG.PROGEQ", "planned")],
            "basis": ["POINTS"],
            "prerequisites": [{"input": "reevaluated_results"}],
            "completion_evidence": ["migrate.prove_equivalence@1"],
        },
    ],
}


def manifest(skip=()):
    ids = [
        ("correlate.run_parallel", 1),
        ("demo.fit_plane", 1),
        ("demo.scan", 2),
        ("import.results_csv", 1),
        ("import.results_dmis", 1),
        ("migrate.fill_form", 1),
        ("migrate.import_form", 1),
        ("migrate.import_point_export", 1),
    ]
    commands = [
        {"id": i, "major": m, "side_effect": "R1"} for i, m in ids if f"{i}@{m}" not in skip
    ]
    return {
        "commands": commands,
        "format": 1,
        "quantity_catalogue_digest": "sha256:" + "0" * 64,
    }


def pretty(obj):
    return (json.dumps(obj, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def check_jcs_subset(obj):
    """The stdlib form below equals RFC 8785 only for ASCII strings (key order by code point
    equals UTF-16 order, no escapes beyond JSON's own) and integers (no float formatting).
    Fail loudly if a later edit leaves that subset."""
    if isinstance(obj, dict):
        for k, v in obj.items():
            check_jcs_subset(k)
            check_jcs_subset(v)
    elif isinstance(obj, list):
        for v in obj:
            check_jcs_subset(v)
    elif isinstance(obj, str):
        # Printable ASCII without quote or backslash: no escaping differences are possible.
        assert all(0x20 <= ord(c) < 0x7F and c not in "\"\\" for c in obj), repr(obj)
    elif isinstance(obj, bool) or obj is None:
        pass
    elif isinstance(obj, int):
        assert -(2**53) < obj < 2**53, obj
    else:
        raise AssertionError(f"outside the JCS subset: {obj!r}")


def jcs(obj):
    check_jcs_subset(obj)
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def outputs():
    forward = copy.deepcopy(BASE)
    forward["stages"][1]["prerequisites"].append({"input": "incumbent_point_export"})
    basis = copy.deepcopy(BASE)
    basis["stages"][3]["basis"] = ["POINTS"]
    digest = "sha256:" + hashlib.sha256(jcs(BASE)).hexdigest() + "\n"
    return {
        "ladder-ok.json": pretty(BASE),
        "forward.json": pretty(forward),
        "basis.json": pretty(basis),
        "manifest-ok.json": pretty(manifest()),
        "manifest-missing.json": pretty(manifest(skip=("migrate.import_form@1",))),
        "ladder-ok.digest": digest.encode("ascii"),
    }


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures (run make_fixtures.py): " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
