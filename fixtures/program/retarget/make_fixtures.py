#!/usr/bin/env python3
"""Generate the fixtures of spec node AUTO.RUN.EXEC.RETGT (the not-re-targetable step guard).

Independent reference (Python standard library only): nothing here runs the Rust code.

Writes:

- `fixtures/program/retarget/manual-pick.json`: a four-step program (s1..s4) in the canonical
  stored form of AUTO.PROG.FMT. Step s4 applies a recorded index-set selection (a manual pick),
  the kind of step AUTO.REC.LIFT marks `retargetable: false`. Parameter `scan` (data_handle) has
  the recorded default `input:scans/bracket-120.ply`; parameter `reference` (data_handle,
  optional) has no default.
- `fixtures/scans/bracket-120-shifted.ply`: the B-120 bracket scan of AUTO.RUN.BIND's generator,
  same seed, every point moved +1 mm in x. Project-authored; its only role is to be hashed.
- `fixtures/program/retarget/expected.json`: the step IDs, the steps marked not re-targetable,
  both scans' SHA-256 (hashlib), and one case per binding with the guard's decision at each step
  key in two settings, HAND-LABELLED from spec/auto/run/exec/retgt/NODE.md:
    `today`     the plan as prepare_run builds it: no stored program carries a recorded default
                digest (AUTO.PROG.FMT stores handles only), so no parameter has one;
    `recorded`  the plan given `recorded_defaults` (scan -> SHA-256 of bracket-120.ply), standing
                in for the digest FMT is to store with the default;
  under the rules:
    REQ-001  a run is re-targeted iff some bound input parameter has no recorded default, or
             its SHA-256 differs from the recorded default's;
    REQ-002  a re-targeted run stops at a step marked `retargetable: false` with
             not_retargetable, naming the step ID, the iteration path and the differing
             parameters (in name order);
    REQ-003  a run that is not re-targeted allows every step.
  A plain reading of those rules (`cross_check`) only guards against a mislabelled case.

`AUTO.PROG.FMT` has no per-step `retargetable` field yet, so the marking of s4 is written here
(`not_retargetable`) and the test applies it to the plan, standing in for AUTO.REC.LIFT. Nothing
here reads the default's dataset to make a recorded digest: what a handle names today is not
evidence of what was recorded.

Canonical JSON: every string here is ASCII and every number a small integer, so json.dumps with
sorted keys and no whitespace is the RFC 8785 form.

CC-BY-4.0 project data. Run:  python fixtures/program/retarget/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
PROGRAM_FILE = HERE / "manual-pick.json"
SCAN_FILE = ROOT / "fixtures/scans/bracket-120.ply"
SHIFTED_FILE = ROOT / "fixtures/scans/bracket-120-shifted.ply"
SCAN = "input:scans/bracket-120.ply"
SHIFTED = "input:scans/bracket-120-shifted.ply"

PROGRAM_ID = "018f0000-0000-7000-8000-000000000300"
STEPS = {
    "s1": "018f0000-0000-7000-8000-000000000301",
    "s2": "018f0000-0000-7000-8000-000000000302",
    "s3": "018f0000-0000-7000-8000-000000000303",
    "s4": "018f0000-0000-7000-8000-000000000304",
}
NOT_RETARGETABLE = ["s4"]
STOP = "not_retargetable"


def canon(v):
    out = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert out.isascii(), "fixture strings must be ASCII"
    return out


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ------------------------------------------------------------------ program


def program():
    def invoke(step, command, args):
        return {"args": args, "command": command, "kind": "invoke", "major": 1, "step_id": STEPS[step]}

    return {
        "format": "fid-program",
        "format_version": 1,
        "main": [
            invoke("s1", "scan.import", {"handle": {"expr": "scan"}}),
            invoke("s2", "align.run_bestfit", {}),
            invoke("s3", "feature.fit_plane", {"points": {"literal": "pc-1"}}),
            invoke("s4", "geo.select_indices", {"indices": {"literal": [101, 102, 103, 117]}}),
        ],
        "meta": {"title": "Manual pick on the bracket"},
        "params": [
            {
                "default": SCAN,
                "description": "Scan of the part (recorded on bracket-120)",
                "name": "scan",
                "required": True,
                "type": "data_handle",
            },
            {
                "description": "Optional reference scan (no recorded default)",
                "name": "reference",
                "required": False,
                "type": "data_handle",
            },
        ],
        "program_id": PROGRAM_ID,
        "subroutines": [],
        "variables": [],
    }


# ------------------------------------------------------------------ scans


def scan_bytes(shift_x, comment):
    """AUTO.RUN.BIND's B-120 scan (fixtures/auto/run/bind/make_cases.py), with every point moved
    `shift_x` mm in x: a 40 x 12 grid over a 160 mm x 48 mm plate minus two holes of radius
    6 mm centred 120 mm apart, LCG noise of at most +-0.02 mm on z, seed 120."""
    state = 120  # seed

    def lcg():
        nonlocal state
        state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
        return state >> 11  # 53 bits

    pts = []
    for j in range(12):
        for i in range(40):
            x = -80 + i * 4 + 2
            y = -24 + j * 4 + 2
            if (x + 60) ** 2 + y ** 2 < 36 or (x - 60) ** 2 + y ** 2 < 36:
                continue
            z_um = (lcg() % 41) - 20  # micrometres, -20..20
            pts.append((x + shift_x, y, z_um))
    lines = [
        "ply",
        "format ascii 1.0",
        "comment " + comment,
        "element vertex %d" % len(pts),
        "property float x",
        "property float y",
        "property float z",
        "end_header",
    ]
    for x, y, z in pts:
        sign = "-" if z < 0 else ""
        lines.append("%d.0000 %d.0000 %s0.%04d" % (x, y, sign, abs(z) * 10))
    return ("\n".join(lines) + "\n").encode("ascii")


# ------------------------------------------------------------------ cases
#
# (name, binding as authored, hand-labelled (retargeted, differing params) for `today`, the same
# for `recorded`). Every case is evaluated at the step keys KEYS; the decision at each key
# follows from the label. Today no parameter has a recorded default, so every bound input
# differs (fail closed).

CASES = [
    ("ac1_recorded_scan_by_default", {}, (True, ["scan"]), (False, [])),
    ("ac1_recorded_scan_bound", {"scan": SCAN}, (True, ["scan"]), (False, [])),
    ("ac2_shifted_scan", {"scan": SHIFTED}, (True, ["scan"]), (True, ["scan"])),
    ("ac3_reference_has_no_default", {"reference": SCAN, "scan": SCAN}, (True, ["reference", "scan"]), (True, ["reference"])),
    ("ac3_shifted_and_no_default", {"reference": SCAN, "scan": SHIFTED}, (True, ["reference", "scan"]), (True, ["reference", "scan"])),
]

# Step keys the guard is asked about: each step outside any loop, and s4 and s1 as if inside
# two nested loops (iteration path outermost first).
KEYS = [
    ("s1", []),
    ("s2", []),
    ("s3", []),
    ("s4", []),
    ("s4", [2, 1]),
    ("s1", [2, 1]),
]


def decision(step, path, retargeted, differing):
    if retargeted and step in NOT_RETARGETABLE:
        return {"code": STOP, "detail": {"iteration_path": path, "params": differing, "step_id": STEPS[step]}}
    return "allow"


def cross_check(binding, digests, defaults):
    """REQ-001 read plainly: the inputs are the bound handles, or the default where unbound."""
    inputs = {}
    for name, default in (("reference", None), ("scan", SCAN)):
        handle = binding.get(name, default)
        if handle is not None:
            inputs[name] = digests[handle]
    differing = sorted(p for p, d in inputs.items() if defaults.get(p) != d)
    return (bool(differing), differing)


def build():
    prog = canon(program()).encode("ascii")
    scan = SCAN_FILE.read_bytes()
    assert scan == scan_bytes(0, "Metrology Maven synthetic fixture: bracket B-120 (AUTO.RUN.BIND), seed 120"), (
        "fixtures/scans/bracket-120.ply is not AUTO.RUN.BIND's scan"
    )
    shifted = scan_bytes(1, "Metrology Maven synthetic fixture: bracket B-120 shifted +1 mm in x (AUTO.RUN.EXEC.RETGT), seed 120")
    digests = {SCAN: sha(scan), SHIFTED: sha(shifted)}
    assert digests[SCAN] != digests[SHIFTED]
    recorded = {"scan": digests[SCAN]}
    cases = []
    for name, binding, today, with_recorded in CASES:
        case = {"binding": binding, "name": name}
        for setting, defaults, label in (("today", {}, today), ("recorded", recorded, with_recorded)):
            got = cross_check(binding, digests, defaults)
            assert got == label, "%s/%s: hand label %s, rules say %s" % (name, setting, label, got)
            retargeted, differing = label
            case[setting] = {
                "decisions": [
                    {"decision": decision(step, path, retargeted, differing), "iteration_path": path, "step": step}
                    for step, path in KEYS
                ],
                "differing": differing,
                "retargeted": retargeted,
            }
        cases.append(case)
    expected = {
        "cases": cases,
        "not_retargetable": NOT_RETARGETABLE,
        "program_digest": sha(prog),
        "program_id": PROGRAM_ID,
        "recorded_defaults": recorded,
        "scan_sha256": {"bracket-120": digests[SCAN], "bracket-120-shifted": digests[SHIFTED]},
        "steps": STEPS,
    }
    return {
        PROGRAM_FILE: prog,
        SHIFTED_FILE: shifted,
        HERE / "expected.json": (json.dumps(expected, indent=1, sort_keys=True) + "\n").encode("ascii"),
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, data in build().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                print("stale: %s" % path.relative_to(ROOT))
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if bad:
        print("run: python fixtures/program/retarget/make_fixtures.py")
        sys.exit(1)


if __name__ == "__main__":
    main()
