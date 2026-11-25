#!/usr/bin/env python3
"""Generate CORE.JRN.UNDO.MOVE fixtures (linear and released) using Python stdlib only.

Outputs (CC-BY-4.0 project data), next to this script:
  linear.json    states s0..s5 on `main` (AC-1, AC-2, AC-3, AC-5)
  released.json  states s0..s4 on `main` where s2 is a W3 release and s4 is not_undoable (AC-4)

Run:   python fixtures/core/jrn/undo/move/make_fixtures.py
Check: python fixtures/core/jrn/undo/move/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def digest(label: str) -> str:
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def make_linear() -> dict:
    # AC-1: states s0..s5 on main (initial state s0 plus 5 undoable W2 commands s1..s5).
    names = ["s0", "s1", "s2", "s3", "s4", "s5"]
    cmds = [
        ("project.create_project", 1, "W2", False),
        ("sketch.add_line", 1, "W2", True),
        ("sketch.add_circle", 1, "W2", True),
        ("feature.fit_plane", 1, "W2", True),
        ("feature.fit_cylinder", 1, "W2", True),
        ("check.eval_distance", 1, "W2", True),
    ]
    entries = []
    prev = None
    for idx, (name, (cmd, major, cls, undoable)) in enumerate(zip(names, cmds), start=1):
        h = digest(f"linear:{name}")
        entries.append(
            {
                "name": name,
                "seq": idx,
                "hash": h,
                "branch": "main",
                "logical_parent": prev,
                "command": cmd,
                "major": major,
                "class": cls,
                "undoable": undoable,
            }
        )
        prev = h
    return {
        "schema": "fid-undo-fixture/1",
        "license": "CC-BY-4.0",
        "seed": 1,
        "branch": "main",
        "entries": entries,
    }


def make_released() -> dict:
    # AC-4: s2 is a W3 release barrier; s4 is a not_undoable W2 command barrier.
    specs = [
        ("s0", "project.create_project", 1, "W2", False),
        ("s1", "feature.fit_plane", 1, "W2", True),
        ("s2", "report.release_record", 1, "W3", False),
        ("s3", "sketch.add_line", 1, "W2", True),
        ("s4", "journal.lock_step", 1, "W2", False),
    ]
    entries = []
    prev = None
    for idx, (name, cmd, major, cls, undoable) in enumerate(specs, start=1):
        h = digest(f"released:{name}")
        entries.append(
            {
                "name": name,
                "seq": idx,
                "hash": h,
                "branch": "main",
                "logical_parent": prev,
                "command": cmd,
                "major": major,
                "class": cls,
                "undoable": undoable,
            }
        )
        prev = h
    return {
        "schema": "fid-undo-fixture/1",
        "license": "CC-BY-4.0",
        "seed": 2,
        "branch": "main",
        "entries": entries,
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        "linear.json": json.dumps(make_linear(), indent=2, sort_keys=True) + "\n",
        "released.json": json.dumps(make_released(), indent=2, sort_keys=True) + "\n",
    }
    for name, content in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != content:
                sys.stderr.write(f"fixture mismatch: {path}\n")
                return 1
        else:
            path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
