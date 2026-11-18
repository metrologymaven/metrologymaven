#!/usr/bin/env python3
"""Validate the AGENT.EVAL.FORMAT fixtures against schema/eval-{task,oracle,suite}-v1.schema.json:
every suite-mini file validates, and every case in fixtures/agent/eval/cases.json gets the
schema verdict it is labelled with ("valid", "invalid"; "n/a" cases are structural and skipped).
Red-team extension: every red-team suite under suite/redteam/ validates, the red-team cases in
fixtures/agent/redteam/*/cases.json get their labelled verdicts, and generators that offer
--check (fixtures/agent/redteam/*/make_*.py) report their fixtures up to date.
Needs: pip install jsonschema
"""

import json
import subprocess
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "agent" / "eval"


def validator(name: str) -> Draft202012Validator:
    schema = json.loads((ROOT / "schema" / name).read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    return Draft202012Validator(schema)


def main() -> int:
    v = {"task": validator("eval-task-v1.schema.json"), "oracle": validator("eval-oracle-v1.schema.json"),
         "suite": validator("eval-suite-v1.schema.json")}
    bad = 0
    suite = FIX / "suite-mini"
    suites = [suite] + sorted(p.parent for p in (ROOT / "suite" / "redteam").glob("*/suite.json"))
    files = []
    for sdir in suites:
        files += [("suite", sdir / "suite.json")]
        files += [("task", p) for p in sorted((sdir / "tasks").glob("*/task.json"))]
        files += [("oracle", p) for p in sorted((sdir / "oracles").glob("*/oracle.json"))]
    for kind, p in files:
        errs = list(v[kind].iter_errors(json.loads(p.read_text(encoding="utf-8"))))
        if errs:
            bad += 1
            print(f"FAIL {p.relative_to(ROOT)}: {errs[0].message}")
    case_dirs = [FIX] + sorted(p.parent for p in (ROOT / "fixtures" / "agent" / "redteam").glob("*/cases.json"))
    for cdir in case_dirs:
        for c in json.loads((cdir / "cases.json").read_text(encoding="utf-8")):
            if c["schema"] == "n/a":
                continue
            doc = json.loads((cdir / "cases" / c["file"]).read_text(encoding="utf-8"))
            ok = v[c["kind"]].is_valid(doc)
            if ok != (c["schema"] == "valid"):
                bad += 1
                print(f"FAIL {cdir.relative_to(ROOT)}/cases/{c['file']}: schema says "
                      f"{'valid' if ok else 'invalid'}, labelled {c['schema']}")
    for gen in sorted((ROOT / "fixtures" / "agent" / "redteam").glob("*/make_*.py")):
        r = subprocess.run([sys.executable, str(gen), "--check"], check=False)
        if r.returncode != 0:
            bad += 1
            print(f"FAIL {gen.relative_to(ROOT)} --check")
    print("eval suite schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
