#!/usr/bin/env python3
"""Generate CORE.PRJ.META fixture `small_project.json` (10 elements, fixed seed, CC-BY-4.0).

Run:   python fixtures/core/prj/meta/make_fixtures.py
Check: python fixtures/core/prj/meta/make_fixtures.py --check
"""

import json
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
NS = uuid.UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8")


def make_small_project() -> dict:
    project_id = str(uuid.uuid5(NS, "core.prj.meta:small_project:seed42"))
    stage_id = str(uuid.uuid5(NS, "core.prj.meta:small_project:stage0"))
    root_id = str(uuid.uuid5(NS, "core.prj.meta:small_project:root"))
    elements = []
    for i in range(10):
        eid = str(uuid.uuid5(NS, f"core.prj.meta:small_project:element:{i}"))
        elements.append(
            {
                "alias": f"e{i}",
                "id": eid,
                "name": f"Feature {i}",
                "type": "geo.plane" if i % 2 == 0 else "geo.cylinder",
            }
        )
    return {
        "schema": "fid-prj-meta-fixture/1",
        "license": "CC-BY-4.0",
        "seed": 42,
        "project_id": project_id,
        "namespace": str(NS),
        "stage_id": stage_id,
        "root_group_id": root_id,
        "elements": elements,
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    content = json.dumps(make_small_project(), indent=2, sort_keys=True) + "\n"
    path = HERE / "small_project.json"
    if check:
        if not path.exists() or path.read_text(encoding="utf-8") != content:
            sys.stderr.write(f"fixture mismatch: {path}\n")
            return 1
        return 0
    path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
