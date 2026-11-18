"""Writes the persona-file fixtures for FLOW.PERSONA.PERS from the shipped file.

Each fixture is the shipped `acceptance/personas/personas.json` with one deliberate
change. This script only produces inputs; the expected findings in `expected.txt` are
labelled by hand from the spec's rules (REQ-FLOW.PERSONA.PERS-001..004), never by running
the Rust check.

Run from the repository root:  python fixtures/flow/personas/make_fixtures.py
Standard library only.
"""

import copy
import json
import pathlib

ROOT = pathlib.Path(__file__).resolve().parents[3]
OUT = pathlib.Path(__file__).resolve().parent
BASE = json.loads((ROOT / "acceptance" / "personas" / "personas.json").read_text(encoding="utf-8"))


def rec(doc, pid):
    for r in doc["personas"]:
        if r["id"] == pid:
            return r
    raise KeyError(pid)


def write(name, doc):
    text = json.dumps(doc, indent=2, ensure_ascii=False) + "\n"
    (OUT / name).write_text(text, encoding="utf-8", newline="\n")


def variant(name, change):
    doc = copy.deepcopy(BASE)
    change(doc)
    write(name, doc)


def drop_07(doc):
    doc["personas"] = [r for r in doc["personas"] if r["id"] != "JOB.PER.07"]


def bad_app(doc):
    rec(doc, "JOB.PER.01")["primary_app"] = "measure"


def bad_profile(doc):
    rec(doc, "JOB.PER.05")["governance_profile"] = "strict"


def extra(doc):
    r = copy.deepcopy(rec(doc, "JOB.PER.09"))
    r["id"] = "JOB.PER.10"
    doc["personas"].append(r)


def duplicate(doc):
    doc["personas"].insert(3, copy.deepcopy(rec(doc, "JOB.PER.03")))


def bad_schema(doc):
    rec(doc, "JOB.PER.01")["nickname"] = "not a field"
    rec(doc, "JOB.PER.02")["role"] = "x" * 121
    rec(doc, "JOB.PER.04")["goals"] = []
    rec(doc, "JOB.PER.05")["goals"] = ["a", "b", "c", "d", "e", "f"]
    rec(doc, "JOB.PER.08")["entry_phase"] = "P5"
    rec(doc, "JOB.PER.09")["environment"] = "home"


def auditor(doc):
    r = rec(doc, "JOB.PER.06")
    r["primary_app"] = "report"
    r["environment"] = "shop_floor"


def wrong_format(doc):
    doc["format"] = "persona-set@2"


def main():
    write("valid.json", BASE)
    variant("missing-07.json", drop_07)
    variant("bad-app.json", bad_app)
    variant("bad-profile.json", bad_profile)
    variant("extra.json", extra)
    variant("duplicate.json", duplicate)
    variant("bad-schema.json", bad_schema)
    variant("auditor.json", auditor)
    variant("wrong-format.json", wrong_format)


if __name__ == "__main__":
    main()
