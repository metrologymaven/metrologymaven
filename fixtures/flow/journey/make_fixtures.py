"""Writes the journey-map fixtures for FLOW.PERSONA.SCHEMA and the digest of the valid map.

Each fixture is a small hand-written `journey-map@1` file, most of them the 12-step
`valid-job.json` with one deliberate change. The maps are fictional: no person is named,
and the owner node IDs are only checked for syntax.

This script is the independent reference for two things:
- the map digest: SHA-256 of the RFC 8785 (JCS) form of the map, written here with the
  standard library only (no floats, ASCII keys, so `json.dumps` with sorted keys and no
  whitespace is the JCS form);
- a second, independent implementation of the structural rules (`reference_findings`),
  used only to cross-check the hand-labelled `expected.txt` and the shipped persona maps.
  The expected findings in `expected.txt` are labelled by hand from the spec's rules
  (REQ-FLOW.PERSONA.SCHEMA-001..005), never by running the Rust validator.

Run from the repository root:  python fixtures/flow/journey/make_fixtures.py [--check]
With --check, nothing is written; the script fails if a committed file differs, if the
reference findings differ from `expected.txt`, or if a shipped map has findings.
Standard library only.
"""

import copy
import hashlib
import json
import pathlib
import re
import sys

OUT = pathlib.Path(__file__).resolve().parent
ROOT = OUT.parents[2]
CHECK = "--check" in sys.argv[1:]
PROBLEMS = []

# ------------------------------------------------------------------ the valid job map

COMMANDS = [
    ("import.cad_step@1", "IO.CAD.STEP.READ", False),
    ("import.cad_pmi@1", "IO.CAD.STEP.PMI", False),
    ("gdt.propose_characteristics_from_pmi@1", "GDT.PMI.PROPOSE", False),
    ("gdt.adopt_characteristics@1", "GDT.PMI.ADOPT", True),
    ("import.mesh_stl@1", "IO.MESH.STL", False),
    ("align.run_bestfit@1", "ALIGN.BESTFIT.RUN", False),
    ("compare.surface_cad@1", "CMP.SURF.CAD", False),
    ("compare.point_create@1", "CMP.POINT.CREATE", False),
    ("compare.point_evaluate@1", "CMP.POINT.EVAL", False),
    ("view.set_colormap@1", "VIEW.MAP.COLOR", False),
    ("report.export_pdf@1", "RPT.EXPORT.PDF", False),
    ("program.from_journal@1", "PROG.REC.JOURNAL", False),
]


def valid_job():
    steps = []
    for i, (cmd, owner, human_only) in enumerate(COMMANDS):
        n = i + 1
        step = {"n": n, "actor": "human", "command": cmd, "owner": owner}
        if n == 4:
            step["decision"] = "Which proposed characteristics to adopt as they are"
        step["handoff"] = n + 1 if n < len(COMMANDS) else "The recorded program goes to program release"
        step["human_only"] = human_only
        steps.append(step)
    return {
        "format": "journey-map@1",
        "map_id": "j1",
        "subject": "JOB.ACC.01",
        "kind": "job",
        "revision": 1,
        "app": "inspect",
        "phase": "P1b",
        "personas": ["JOB.PER.02"],
        "steps": steps,
    }


def valid_persona():
    return {
        "format": "journey-map@1",
        "map_id": "demo-persona",
        "subject": "JOB.PER.03",
        "kind": "persona",
        "revision": 2,
        "app": "inspect",
        "phase": "P1b",
        "personas": ["JOB.PER.03", "JOB.PER.02"],
        "exercised_by": ["JOB.ACC.05"],
        "steps": [
            {"n": 1, "actor": "operator", "screen": "SCR.KIOSK.LOGIN", "owner": "UX.SCREENS.KIOSK",
             "handoff": 2, "human_only": False},
            {"n": 2, "actor": "operator", "command": "run.start_program@1", "owner": "CMM.EXEC.RUN",
             "decision": "Whether the part on the table matches the serial shown", "handoff": 3,
             "human_only": False, "phase": "P2"},
            {"n": 3, "actor": "system", "screen": "SCR.KIOSK.RESULT", "owner": "UX.SCREENS.KIOSK",
             "handoff": "A part that did not pass goes to JOB.PER.02 for review", "human_only": False},
        ],
    }


# ------------------------------------------------------------------ fixtures

def step(d, n):
    return d["steps"][n - 1]


def agent_humanonly(d):
    step(d, 4)["actor"] = "agent"


def gap(d):
    d["steps"] = [copy.deepcopy(step(d, k)) for k in (1, 2, 4)]
    d["steps"][1]["handoff"] = 4
    d["steps"][2]["handoff"] = "Adopted characteristics go on to alignment"


def both(d):
    step(d, 3)["screen"] = "SCR.GDT.PROPOSALS"


def neither(d):
    del step(d, 5)["command"]


def unknown_field(d):
    d["owner_note"] = "not part of the format"
    step(d, 2)["note"] = "not part of the format"


def bad_handoff(d):
    step(d, 6)["handoff"] = 9
    step(d, 12)["handoff"] = 13


def duplicate_n(d):
    step(d, 7)["n"] = 6


def bad_fields(d):
    d["format"] = "journey-map@2"
    d["kind"] = "flow"
    d["revision"] = 0
    d["phase"] = "P5"
    step(d, 1)["command"] = "Import.CAD@1"
    step(d, 2)["owner"] = "core.jrn"
    step(d, 3)["actor"] = "robot"
    step(d, 5)["human_only"] = "yes"
    step(d, 8)["phase"] = "P9"


def kind_subject(d):
    d["subject"] = "JOB.PER.02"


def exercised_on_job(d):
    d["exercised_by"] = ["JOB.ACC.01"]


def persona_no_exercised(d):
    del d["exercised_by"]


def persona_subject_missing(d):
    d["personas"] = ["JOB.PER.02"]


def unknown_persona(d):
    step(d, 4)["handoff"] = "JOB.PER.05 reviews the adopted characteristics"


def dup_persona(d):
    d["personas"] = ["JOB.PER.02", "JOB.PER.02"]


def empty_steps(d):
    d["steps"] = []


def bad_map_id(d):
    d["map_id"] = "J1 map"


def use_subject(d):
    d["subject"] = "JOB.USE.02"


def whole_floats(d):
    d["revision"] = 3.0
    step(d, 2)["n"] = 2.0
    step(d, 1)["handoff"] = 2.0


def fractions(d):
    d["revision"] = 1.5
    step(d, 2)["n"] = 2.5


FIXTURES = [
    ("valid-job.json", valid_job, None),
    ("valid-persona.json", valid_persona, None),
    ("agent-humanonly.json", valid_job, agent_humanonly),
    ("gap.json", valid_job, gap),
    ("both.json", valid_job, both),
    ("neither.json", valid_job, neither),
    ("unknown-field.json", valid_job, unknown_field),
    ("bad-handoff.json", valid_job, bad_handoff),
    ("duplicate-n.json", valid_job, duplicate_n),
    ("bad-fields.json", valid_job, bad_fields),
    ("kind-subject.json", valid_job, kind_subject),
    ("exercised-on-job.json", valid_job, exercised_on_job),
    ("persona-no-exercised.json", valid_persona, persona_no_exercised),
    ("persona-subject-missing.json", valid_persona, persona_subject_missing),
    ("unknown-persona.json", valid_job, unknown_persona),
    ("dup-persona.json", valid_job, dup_persona),
    ("empty-steps.json", valid_job, empty_steps),
    ("bad-map-id.json", valid_job, bad_map_id),
    ("job-use-subject.json", valid_job, use_subject),
    ("whole-floats.json", valid_job, whole_floats),
    ("fractions.json", valid_job, fractions),
]

# A duplicate key cannot be written with json.dumps; this one repeats `app`.
DUP_KEY = (
    '{"format": "journey-map@1", "map_id": "j1", "subject": "JOB.ACC.01", "kind": "job",\n'
    ' "revision": 1, "app": "inspect", "app": "report", "phase": "P1b", "personas": [],\n'
    ' "steps": [{"n": 1, "actor": "human", "command": "import.cad_step@1",\n'
    '            "owner": "IO.CAD.STEP.READ", "handoff": "done", "human_only": false}]}\n'
)


def pretty(doc):
    return (json.dumps(doc, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def jcs(doc):
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def emit(name, data):
    path = OUT / name
    if CHECK:
        if not path.exists() or path.read_bytes() != data:
            PROBLEMS.append(f"{name} differs from the reference; run make_fixtures.py")
    else:
        path.write_bytes(data)


# ------------------------------------------------------------------ independent reference rules

PHASES = {"P0", "P1a", "P1b", "P2", "P3", "P4", "later"}
ACTORS = {"human", "operator", "admin", "agent", "system"}
GROUP = {"persona": {"PER"}, "job": {"ACC", "USE"}, "setup": {"USE"}}
TOP = {"format", "map_id", "subject", "kind", "revision", "app", "phase", "personas", "steps"}
STEP_FIELDS = {"n", "actor", "command", "screen", "owner", "handoff", "decision", "human_only", "phase"}
IDENT = re.compile(r"[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?")
JOB = re.compile(r"JOB\.([A-Z]{1,8})\.[0-9]{2}")
NODE = re.compile(r"[A-Z][A-Z0-9]*(?:\.[A-Z][A-Z0-9]*)*")
COMMAND = re.compile(r"[a-z0-9](?:[a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}")
SCREEN = re.compile(r"[A-Za-z][A-Za-z0-9._-]{0,127}")
MENTION = re.compile(r"(?<![A-Za-z0-9.])JOB\.PER\.[0-9]{2}(?![A-Za-z0-9])")


def full(rx, s):
    return isinstance(s, str) and rx.fullmatch(s) is not None


def count(v):
    # A non-negative whole number up to 2**53 - 1. `2.0` counts as 2, as in the Rust validator
    # (fid-canon reads every number as a double) and in JSON Schema's `integer`; RFC 8785
    # writes both as `2`, so the digest cannot tell them apart either.
    if isinstance(v, bool) or not isinstance(v, (int, float)):
        return False
    return v >= 0 and float(v).is_integer() and v <= 2**53 - 1


def text(v):
    return isinstance(v, str) and 0 < len(v.encode("utf-8")) <= 256 and not any(
        ord(c) < 0x20 or 0x7F <= ord(c) <= 0x9F for c in v)


def no_dups(pairs):
    seen = set()
    for k, _ in pairs:
        if k in seen:
            raise ValueError("duplicate key " + k)
        seen.add(k)
    return dict(pairs)


def reference_findings(raw):
    """Findings as `<CODE> <map_id> [step <k>] <field>` lines, sorted like the validator."""
    try:
        d = json.loads(raw.decode("utf-8"), object_pairs_hook=no_dups)
    except ValueError:
        return ["MAP-SCHEMA - -"]
    out = []
    mid = d.get("map_id") if full(IDENT, d.get("map_id")) else "-"

    def bad(code, field, k=-1):
        out.append((code, mid, k, field))

    for key in d:
        if key not in TOP and key != "exercised_by":
            bad(1, key)
    if d.get("format") != "journey-map@1":
        bad(1, "format")
    if mid == "-":
        bad(1, "map_id")
    kind = d.get("kind") if d.get("kind") in GROUP else None
    if kind is None:
        bad(1, "kind")
    subj = d.get("subject")
    m = JOB.fullmatch(subj) if isinstance(subj, str) else None
    if not m:
        bad(1, "subject")
    elif kind and m.group(1) not in GROUP[kind]:
        bad(1, "subject")
    if not (count(d.get("revision")) and d["revision"] >= 1):
        bad(1, "revision")
    if not full(IDENT, d.get("app")):
        bad(1, "app")
    if d.get("phase") not in PHASES:
        bad(1, "phase")

    def ids(key, group):
        v = d.get(key)
        if not isinstance(v, list):
            bad(1, key)
            return None
        ok = True
        for i, x in enumerate(v):
            mm = JOB.fullmatch(x) if isinstance(x, str) else None
            if not mm or mm.group(1) != group or x in v[:i]:
                bad(1, f"{key}[{i}]")
                ok = False
        return v if ok else None

    personas = ids("personas", "PER")
    if kind == "persona" and personas is not None and subj not in personas:
        bad(1, "personas")
    if kind == "persona":
        ids("exercised_by", "ACC")
    elif kind and "exercised_by" in d:
        bad(1, "exercised_by")
    steps = d.get("steps")
    if not isinstance(steps, list) or not steps:
        bad(1, "steps")
        steps = []
    if len(steps) > 500:
        out.append((2, mid, -1, "steps"))
        steps = []
    for i, s in enumerate(steps):
        k = i + 1
        for key in s:
            if key not in STEP_FIELDS:
                bad(1, key, k)
        n = s.get("n")
        if not (count(n) and n >= 1):
            bad(1, "n", k)
        elif n != k:
            out.append((3, mid, k, "n"))
        if s.get("actor") not in ACTORS:
            bad(1, "actor", k)
        if ("command" in s) == ("screen" in s):
            bad(1, "-", k)
        elif "command" in s and not full(COMMAND, s["command"]):
            bad(1, "command", k)
        elif "screen" in s and not full(SCREEN, s["screen"]):
            bad(1, "screen", k)
        if not (full(NODE, s.get("owner")) and len(s["owner"]) <= 128):
            bad(1, "owner", k)
        h = s.get("handoff")
        if count(h) and h >= 1:
            nxt = steps[i + 1].get("n") if i + 1 < len(steps) else None
            if i + 1 >= len(steps) or (nxt == k + 1 and count(nxt) and nxt != h):
                out.append((3, mid, k, "handoff"))
        elif not text(h):
            bad(1, "handoff", k)
        if "decision" in s and not text(s["decision"]):
            bad(1, "decision", k)
        if not isinstance(s.get("human_only"), bool):
            bad(1, "human_only", k)
        if "phase" in s and s["phase"] not in PHASES:
            bad(1, "phase", k)
        if personas is not None:
            for field in ("handoff", "decision"):
                if isinstance(s.get(field), str):
                    for pid in MENTION.findall(s[field]):
                        if pid not in personas:
                            bad(1, field, k)
        if s.get("human_only") is True and s.get("actor") == "agent":
            out.append((0, mid, k, "actor"))
    names = ["MAP-AGENT-HUMANONLY", "MAP-SCHEMA", "MAP-SIZE", "MAP-STEP-ORDER"]
    lines = []
    for code, m_id, k, field in sorted(set(out)):
        where = f" step {k}" if k >= 0 else ""
        lines.append(f"{names[code]} {m_id}{where} {field}")
    return lines


def expected_rows():
    rows = {}
    for line in (OUT / "expected.txt").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#"):
            continue
        name, label = line.split("\t")
        rows.setdefault(name, [])
        if label != "ok":
            rows[name].append(label)
    return rows


# ------------------------------------------------------------------ main

def main():
    docs = {}
    for name, base, change in FIXTURES:
        doc = base()
        if change:
            change(doc)
        docs[name] = pretty(doc)
        emit(name, docs[name])
    docs["dup-key.json"] = DUP_KEY.encode("utf-8")
    emit("dup-key.json", docs["dup-key.json"])
    emit("valid-job.digest", ("sha256:" + hashlib.sha256(jcs(valid_job())).hexdigest() + "\n").encode("ascii"))
    if CHECK:
        rows = expected_rows()
        if sorted(rows) != sorted(docs):
            PROBLEMS.append("expected.txt does not list exactly the fixtures")
        for name, raw in sorted(docs.items()):
            got = reference_findings(raw)
            if got != rows.get(name):
                PROBLEMS.append(f"{name}: reference gives {got}, expected.txt says {rows.get(name)}")
        for path in sorted((ROOT / "acceptance").rglob("*.json")):
            raw = path.read_bytes()
            try:
                doc = json.loads(raw.decode("utf-8"))
            except ValueError:
                continue
            if isinstance(doc, dict) and doc.get("format") == "journey-map@1":
                got = reference_findings(raw)
                if got:
                    PROBLEMS.append(f"{path.relative_to(ROOT).as_posix()}: {got}")
    for p in PROBLEMS:
        print(p, file=sys.stderr)
    return 1 if PROBLEMS else 0


if __name__ == "__main__":
    sys.exit(main())
