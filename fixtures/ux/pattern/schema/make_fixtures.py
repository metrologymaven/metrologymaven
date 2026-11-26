#!/usr/bin/env python3
"""Generate and verify the synthetic fixtures of spec node UX.PATTERN.SCHEMA (fid-design pattern-check).

License: CC-BY-4.0 (synthetic fixtures).

Writes, below this directory:
- ok/ok.md: a complete pattern-spec@1 file (AC-1);
- no-cancel.md, bad-comp.md, stale-cmd.md: the AC-2 .. AC-4 defects;
- extra/*.md: one defect each for the other rules;
- pkg/: the package fixture: package.toml and components/ holding two component-spec@1 files
  (button.md copied byte for byte from fixtures/ux/comp/schema/ok/button.md, field.md the same
  spec with its id and title changed) and ghost.md, a file that names comp.ghost but is not a
  component-spec@1 (wrong format), so it declares no component;
- manifest.json: a CORE.REG.MAN-shaped manifest (only `id` and `major` are read by the check);
- journeys/: two well-formed journey-map@1 files; journeys-bad/: one good map plus a duplicate
  map_id, a file of another format and a step numbered 0;
- expected.json: per case, the (code, subject) errors and warnings and the unchecked list;
- glob-cases.json: command patterns against command IDs, with the expected match.

The expectations are hand-labelled here from the rules of the spec node (HAND below), never from
the Rust output. An independent reference checker written from the spec in this file (stdlib only:
tomllib, json, re) recomputes every case, and generation stops if it disagrees with a hand label.
Glob matches are computed by translating each pattern into a Python regular expression, a
different method from the Rust matcher.

Usage:
    python fixtures/ux/pattern/schema/make_fixtures.py          # write
    python fixtures/ux/pattern/schema/make_fixtures.py --check  # verify, exit 1 on drift
"""

from __future__ import annotations

import json
import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
BUTTON_SRC = REPO / "fixtures" / "ux" / "comp" / "schema" / "ok" / "button.md"

OK_TOML = """\
format = "pattern-spec@1"
id = "pat.command-run"
title = "Run a command from the palette"
trigger = "The person presses Ctrl+K or chooses Run command from the menu."
states = ["closed", "filtering", "running", "done"]
commands = ["view.select_*@1", "palette.run_command@1"]
cancel = ["Escape closes the palette; no command runs and the selection is unchanged."]
failure = ["A refused command shows its error code and one suggested fix inside the palette."]

[[steps]]
n = 1
actor = "human"
components = ["comp.field"]
input = "Text that names the command."

[[steps]]
n = 2
actor = "human"
components = ["comp.field", "comp.button"]
input = "The choice of one matching command."

[[steps]]
n = 3
actor = "system"
components = []
input = "None; the command bus runs the chosen command."

[[keyboard]]
step = 1
keys = "Ctrl+K, then type"

[[keyboard]]
step = 2
keys = "Up and Down, then Enter"
note = "Enter on a command that needs parameters opens its form."

[[journeys]]
map = "fx-run"
steps = [2, 3]

[[journeys]]
map = "fx-review"
steps = [1]
"""

PROSE = """\

# Run a command from the palette (synthetic fixture)

The palette lists the commands that match the typed text; choosing one runs it once.
"""

MANIFEST = {
    "format": 1,
    "quantity_catalogue_digest": "sha256:" + "0" * 64,
    "commands": [
        {"id": "journal.undo_last", "major": 1},
        {"id": "palette.open", "major": 1},
        {"id": "palette.run_command", "major": 1},
        {"id": "report.export_pdf", "major": 2},
        {"id": "view.select_all", "major": 1},
        {"id": "view.select_one", "major": 1},
    ],
}


def journey(map_id: str, n_steps: int) -> dict:
    return {
        "format": "journey-map@1",
        "map_id": map_id,
        "subject": "JOB.USE.01",
        "kind": "job",
        "revision": 1,
        "app": "inspect",
        "phase": "P1a",
        "personas": ["JOB.PER.01"],
        "steps": [
            {"n": i, "actor": "human", "command": "palette.run_command@1", "owner": "UX.PATTERN.CMDFLOW",
             "handoff": None, "human_only": False}
            for i in range(1, n_steps + 1)
        ],
    }


def spec(toml: str, prose: str = PROSE) -> str:
    return "```toml\n" + toml + "```\n" + prose


def edit(text: str, old: str, new: str) -> str:
    """Replace `old` (which must occur exactly once) by `new`."""
    if text.count(old) != 1:
        raise SystemExit(f"fixture edit: {old!r} occurs {text.count(old)} times")
    return text.replace(old, new)


# ---------------------------------------------------------------- the reference checker
# Written from spec node UX.PATTERN.SCHEMA and the pattern-spec@1 format stated in the Rust
# module docs; it shares no code with the Rust implementation.

ACTORS = {"human", "operator", "admin", "agent", "system"}
PERSONS = {"human", "operator", "admin"}
TOP = {"format", "id", "title", "trigger", "steps", "states", "commands", "keyboard", "cancel", "failure", "journeys"}
IDENT = re.compile(r"[a-z][a-z0-9-]*\Z")
NAME = re.compile(r"[a-z0-9]([a-z0-9_.]*[a-z0-9])?\Z")


def ident(s: object) -> bool:
    return isinstance(s, str) and len(s) <= 64 and IDENT.match(s) is not None


def text_ok(s: object) -> bool:
    return isinstance(s, str) and s.strip() != "" and len(s) <= 1000 and not any(
        ord(ch) < 0x20 or 0x7F <= ord(ch) <= 0x9F for ch in s
    )


def comp_id(s: object) -> bool:
    return isinstance(s, str) and s.startswith("comp.") and ident(s[5:])


def pattern_ok(s: str) -> bool:
    if len(s.encode()) > 128 or "@" not in s:
        return False
    glob, major = s.rsplit("@", 1)
    if not re.fullmatch(r"[1-9][0-9]{0,4}", major):
        return False
    return (re.fullmatch(r"[a-z0-9][a-z0-9_.*]*", glob) is not None and "**" not in glob
            and glob.count("*") <= 8)


def pattern_regex(s: str) -> re.Pattern:
    glob, major = s.rsplit("@", 1)
    body = "".join("[a-z0-9_.]*" if ch == "*" else re.escape(ch) for ch in glob)
    return re.compile(body + "@" + str(int(major)) + r"\Z")


def front(text: str):
    lines = text.splitlines()
    if not lines or lines[0].rstrip() != "```toml":
        return None
    for i in range(1, len(lines)):
        if lines[i].rstrip() == "```":
            return "\n".join(lines[1:i]) + "\n"
    return None


def is_int(v: object) -> bool:
    return isinstance(v, int) and not isinstance(v, bool)


def ref_spec(name: str, text: str):
    """(errors, refs) of one spec on its own; errors are (code, subject) pairs."""
    err: set[tuple[str, str]] = set()
    refs = {"components": [], "commands": [], "journeys": [], "id": None}
    block = front(text)
    if block is None:
        return {("PAT-SCHEMA", name)}, refs
    try:
        doc = tomllib.loads(block)
    except tomllib.TOMLDecodeError:
        return {("PAT-SCHEMA", name)}, refs
    if set(doc) - TOP:
        err.add(("PAT-SCHEMA", name))
    if doc.get("format") != "pattern-spec@1":
        return err | {("PAT-SCHEMA", name)}, refs
    pid = doc.get("id")
    if isinstance(pid, str) and pid.startswith("pat.") and ident(pid[4:]):
        refs["id"] = pid
    else:
        err.add(("PAT-SCHEMA", name))
    if "title" in doc and not text_ok(doc["title"]):
        err.add(("PAT-SCHEMA", name))
    if not text_ok(doc.get("trigger")):
        err.add(("PAT-SCHEMA", name))
    # steps
    steps = doc.get("steps")
    person_steps = None
    if isinstance(steps, list) and steps and all(isinstance(s, dict) for s in steps):
        good = True
        person_steps = set()
        for i, s in enumerate(steps, start=1):
            if set(s) != {"n", "actor", "components", "input"}:
                err.add(("PAT-SCHEMA", name))
                good = False
                continue
            if not (is_int(s["n"]) and s["n"] == i):
                err.add(("PAT-SCHEMA", name))
                good = False
            if s["actor"] not in ACTORS:
                err.add(("PAT-SCHEMA", name))
                good = False
            elif s["actor"] in PERSONS:
                person_steps.add(i)
            if not text_ok(s["input"]):
                err.add(("PAT-SCHEMA", name))
            comps = s["components"]
            if not isinstance(comps, list):
                err.add(("PAT-SCHEMA", name))
                continue
            for j, c in enumerate(comps):
                if comp_id(c):
                    refs["components"].append(c)
                elif isinstance(c, str):
                    err.add(("PAT-COMPONENT-UNKNOWN", f"steps[{i - 1}].components[{j}]"))
                else:
                    err.add(("PAT-SCHEMA", name))
        if not good:
            person_steps = None
    else:
        err.add(("PAT-SCHEMA", name))
    # states
    states = doc.get("states")
    if not (isinstance(states, list) and states and all(ident(s) for s in states)
            and len(set(states)) == len(states)):
        err.add(("PAT-SCHEMA", name))
    # commands
    cmds = doc.get("commands")
    if not isinstance(cmds, list):
        err.add(("PAT-SCHEMA", name))
    else:
        seen = set()
        for c in cmds:
            if not isinstance(c, str) or not pattern_ok(c) or c in seen:
                err.add(("PAT-SCHEMA", name))
                continue
            seen.add(c)
            refs["commands"].append(c)
    # keyboard
    kb = doc.get("keyboard")
    if not isinstance(kb, list) or not kb:
        if kb is not None and not isinstance(kb, list):
            err.add(("PAT-SCHEMA", name))
        else:
            err.add(("PAT-KEYBOARD-MISSING", name))
    else:
        covered = set()
        for k in kb:
            if not (isinstance(k, dict) and {"step", "keys"} <= set(k) <= {"step", "keys", "note"}):
                err.add(("PAT-SCHEMA", name))
                continue
            if not (is_int(k["step"]) and 1 <= k["step"] <= 100000) or not text_ok(k["keys"]):
                err.add(("PAT-SCHEMA", name))
                continue
            if "note" in k and not text_ok(k["note"]):
                err.add(("PAT-SCHEMA", name))
            if person_steps is not None and k["step"] > len(steps):
                err.add(("PAT-SCHEMA", name))
            covered.add(k["step"])
        if person_steps is not None and person_steps - covered:
            err.add(("PAT-KEYBOARD-MISSING", name))
    # cancel, failure
    for key, code in [("cancel", "PAT-CANCEL-MISSING"), ("failure", "PAT-FAILURE-MISSING")]:
        v = doc.get(key)
        if v is None:
            err.add((code, name))
            continue
        if not isinstance(v, list):
            err.add(("PAT-SCHEMA", name))
            err.add((code, name))
            continue
        lines = [x for x in v if text_ok(x)]
        if len(lines) != len(v):
            err.add(("PAT-SCHEMA", name))
        if not lines:
            err.add((code, name))
    # journeys
    js = doc.get("journeys")
    if not isinstance(js, list) or not js:
        err.add(("PAT-SCHEMA", name))
    else:
        maps = set()
        for j in js:
            if not (isinstance(j, dict) and set(j) == {"map", "steps"}):
                err.add(("PAT-SCHEMA", name))
                continue
            st = j["steps"]
            nums = []
            if not (isinstance(st, list) and st):
                err.add(("PAT-SCHEMA", name))
            else:
                for n in st:
                    if is_int(n) and 1 <= n <= 100000 and n not in nums:
                        nums.append(n)
                    else:
                        err.add(("PAT-SCHEMA", name))
            if not ident(j["map"]):
                err.add(("PAT-SCHEMA", name))
                continue
            if j["map"] in maps:
                err.add(("PAT-SCHEMA", name))
            maps.add(j["map"])
            refs["journeys"].append((j["map"], nums))
    return err, refs


def ref_components(pkg: Path) -> set[str]:
    out = set()
    d = pkg / "components"
    if not d.is_dir():
        return out
    for p in sorted(d.glob("*.md")):
        block = front(p.read_text(encoding="utf-8"))
        if block is None:
            continue
        try:
            doc = tomllib.loads(block)
        except tomllib.TOMLDecodeError:
            continue
        if doc.get("format") == "component-spec@1" and comp_id(doc.get("id")):
            out.add(doc["id"])
    return out


def ref_journeys(d: Path):
    maps: dict[str, set[int]] = {}
    err = set()
    for p in sorted(d.glob("*.json"), key=lambda q: q.name.encode()):
        try:
            v = json.loads(p.read_text(encoding="utf-8"))
        except ValueError:
            err.add(("PAT-SCHEMA", p.name))
            continue
        ok = (isinstance(v, dict) and v.get("format") == "journey-map@1" and ident(v.get("map_id"))
              and isinstance(v.get("steps"), list))
        if ok:
            ns = [s.get("n") if isinstance(s, dict) else None for s in v["steps"]]
            ok = all(is_int(n) and n >= 1 for n in ns) and len(set(ns)) == len(ns)
        if not ok or v["map_id"] in maps:
            err.add(("PAT-SCHEMA", p.name))
            continue
        maps[v["map_id"]] = set(ns)
    return maps, err


def reference(files: dict[str, str], case: dict):
    """Recompute a case: (errors, warnings, unchecked)."""
    root = case["root"]
    if root in files:
        specs = {Path(root).name: files[root]}
    else:
        prefix = root.rstrip("/") + "/"
        specs = {k[len(prefix):]: v for k, v in sorted(files.items())
                 if k.startswith(prefix) and "/" not in k[len(prefix):]}
    comps = ref_components(HERE / case["package"])
    commands = None
    if case.get("manifest"):
        m = json.loads((HERE / case["manifest"]).read_text(encoding="utf-8"))
        commands = [f"{c['id']}@{c['major']}" for c in m["commands"]]
    maps = None
    err: set[tuple[str, str]] = set()
    warn: set[tuple[str, str]] = set()
    if case.get("journeys"):
        maps, jerr = ref_journeys(HERE / case["journeys"])
        err |= jerr
    ids = set()
    for name, text in sorted(specs.items()):
        e, refs = ref_spec(name, text)
        err |= e
        for c in refs["components"]:
            if c not in comps:
                err.add(("PAT-COMPONENT-UNKNOWN", c))
        if commands is not None:
            for c in refs["commands"]:
                rx = pattern_regex(c)
                if not any(rx.match(x) for x in commands):
                    warn.add(("PAT-COMMAND-UNMATCHED", c))
        if maps is not None:
            for m, nums in refs["journeys"]:
                if m not in maps:
                    err.add(("PAT-JOURNEY-UNKNOWN", m))
                else:
                    for n in nums:
                        if n not in maps[m]:
                            err.add(("PAT-JOURNEY-UNKNOWN", f"{m}/{n}"))
        if refs["id"] is not None:
            if refs["id"] in ids:
                err.add(("PAT-SCHEMA", name))
            ids.add(refs["id"])
    unchecked = [u for u, given in [("commands", commands is not None), ("journeys", maps is not None)] if not given]
    return err, warn, unchecked


# ---------------------------------------------------------------- the cases


def cases() -> tuple[dict[str, str], list[dict]]:
    """Spec files (path -> text) and the hand-labelled expectations."""
    files: dict[str, str] = {}
    expect: list[dict] = []
    M, J = "manifest.json", "journeys"

    def add(path: str, text: str | None, errors, warnings=(), manifest=M, journeys=J, package="pkg"):
        if text is not None:
            files[path] = text
        unchecked = [u for u, g in [("commands", manifest), ("journeys", journeys)] if not g]
        expect.append({
            "root": path, "package": package, "manifest": manifest, "journeys": journeys,
            "errors": sorted([list(e) for e in errors]), "warnings": sorted([list(w) for w in warnings]),
            "unchecked": unchecked,
        })

    ok = spec(OK_TOML)
    # AC-1: every rule holds, with the manifest (as the AC runs it) and with the journeys too.
    add("ok/ok.md", ok, [], journeys=None)
    add("ok/ok.md", None, [])
    add("ok", None, [])
    # Without --manifest nothing about commands is claimed.
    add("ok/ok.md", None, [], manifest=None, journeys=None)
    # AC-2: the cancel path is empty.
    add("no-cancel.md", spec(edit(OK_TOML, 'cancel = ["Escape closes the palette; no command runs and the selection is unchanged."]',
                                  "cancel = []")), [["PAT-CANCEL-MISSING", "no-cancel.md"]])
    # AC-3: a component that is no component spec of the package.
    add("bad-comp.md", spec(edit(OK_TOML, 'components = ["comp.field", "comp.button"]', 'components = ["comp.field", "comp.nope"]')),
        [["PAT-COMPONENT-UNKNOWN", "comp.nope"]])
    # AC-4: a stale command pattern is a warning, not an error.
    add("stale-cmd.md", spec(edit(OK_TOML, 'commands = ["view.select_*@1", "palette.run_command@1"]', 'commands = ["foo.bar_*@1"]')),
        [], [["PAT-COMMAND-UNMATCHED", "foo.bar_*@1"]])

    # Further rules, one defect each.
    add("extra/no-cancel-key.md", spec(edit(OK_TOML, 'cancel = ["Escape closes the palette; no command runs and the selection is unchanged."]\n', "")),
        [["PAT-CANCEL-MISSING", "no-cancel-key.md"]])
    add("extra/no-failure.md", spec(edit(OK_TOML, 'failure = ["A refused command shows its error code and one suggested fix inside the palette."]',
                                         'failure = ["   "]')),
        [["PAT-FAILURE-MISSING", "no-failure.md"], ["PAT-SCHEMA", "no-failure.md"]])
    no_kb = edit(OK_TOML, '[[keyboard]]\nstep = 1\nkeys = "Ctrl+K, then type"\n\n[[keyboard]]\nstep = 2\nkeys = "Up and Down, then Enter"\n'
                 'note = "Enter on a command that needs parameters opens its form."\n\n', "")
    no_kb = edit(no_kb, 'commands = ["view.select_*@1", "palette.run_command@1"]\n',
                 'commands = ["view.select_*@1", "palette.run_command@1"]\nkeyboard = []\n')
    add("extra/no-keyboard.md", spec(no_kb), [["PAT-KEYBOARD-MISSING", "no-keyboard.md"]])
    gap = edit(OK_TOML, '\n[[keyboard]]\nstep = 2\nkeys = "Up and Down, then Enter"\n'
               'note = "Enter on a command that needs parameters opens its form."\n', "")
    add("extra/keyboard-gap.md", spec(gap), [["PAT-KEYBOARD-MISSING", "keyboard-gap.md"]])
    # A system step needs no key; an agent step neither.
    agent = edit(OK_TOML, 'n = 3\nactor = "system"', 'n = 3\nactor = "agent"')
    add("extra/agent-step.md", spec(edit(agent, 'id = "pat.command-run"', 'id = "pat.agent-run"')), [])
    add("extra/ghost-comp.md", spec(edit(OK_TOML, 'components = ["comp.field"]', 'components = ["comp.ghost"]')),
        [["PAT-COMPONENT-UNKNOWN", "comp.ghost"]])
    add("extra/comp-field.md", spec(edit(OK_TOML, 'components = ["comp.field"]', 'components = ["Comp.Field"]')),
        [["PAT-COMPONENT-UNKNOWN", "steps[0].components[0]"]])
    add("extra/journey-map.md", spec(edit(OK_TOML, 'map = "fx-review"', 'map = "fx-nope"')),
        [["PAT-JOURNEY-UNKNOWN", "fx-nope"]])
    add("extra/journey-step.md", spec(edit(OK_TOML, "steps = [2, 3]", "steps = [2, 9]")),
        [["PAT-JOURNEY-UNKNOWN", "fx-run/9"]])
    add("extra/no-front-block.md", "# Palette\n\n" + spec(OK_TOML), [["PAT-SCHEMA", "no-front-block.md"]])
    add("extra/unknown-key.md", spec(edit(OK_TOML, 'title = "Run a command from the palette"\n',
                                          'title = "Run a command from the palette"\ncolour = "blue"\n')),
        [["PAT-SCHEMA", "unknown-key.md"]])
    add("extra/bad-pattern.md", spec(edit(OK_TOML, '"palette.run_command@1"]', '"Palette.Run@1"]')),
        [["PAT-SCHEMA", "bad-pattern.md"]])
    add("extra/wrong-major.md", spec(edit(OK_TOML, '"palette.run_command@1"]', '"palette.run_command@2"]')),
        [], [["PAT-COMMAND-UNMATCHED", "palette.run_command@2"]])
    add("extra/bad-actor.md", spec(edit(OK_TOML, 'n = 3\nactor = "system"', 'n = 3\nactor = "robot"')),
        [["PAT-SCHEMA", "bad-actor.md"]])
    add("extra/step-order.md", spec(edit(OK_TOML, "n = 3\n", "n = 4\n")), [["PAT-SCHEMA", "step-order.md"]])
    add("extra/no-states.md", spec(edit(OK_TOML, 'states = ["closed", "filtering", "running", "done"]', "states = []")),
        [["PAT-SCHEMA", "no-states.md"]])
    # The journeys-bad directory: a duplicate map_id, another format and a step 0 are findings;
    # the good map still resolves the AC-1 references.
    add("ok/ok.md", None, [["PAT-SCHEMA", "b.json"], ["PAT-SCHEMA", "c.json"], ["PAT-SCHEMA", "d.json"],
                           ["PAT-JOURNEY-UNKNOWN", "fx-review"]], journeys="journeys-bad")
    return files, expect


def pretty(obj: object) -> str:
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def package_files() -> dict[str, bytes]:
    button = BUTTON_SRC.read_bytes()
    field = button.decode()
    field = edit(field, 'id = "comp.button"\ntitle = "Button"', 'id = "comp.field"\ntitle = "Text field"')
    ghost = edit(button.decode(), 'format = "component-spec@1"\nid = "comp.button"', 'format = "component-spec@0"\nid = "comp.ghost"')
    toml = (
        'format = "fid-design-package@1"\nversion = "1.0.0"\n'
        'token_format = "dtcg-second-editors-draft-2022-06-14"\nproduct_name = "{{FAMILY_NAME}}"\n'
    )
    return {
        "pkg/package.toml": toml.encode(),
        "pkg/components/button.md": button,
        "pkg/components/field.md": field.encode(),
        "pkg/components/ghost.md": ghost.encode(),
    }


def journey_files() -> dict[str, bytes]:
    out = {
        "journeys/fx-run.json": pretty(journey("fx-run", 4)),
        "journeys/fx-review.json": pretty(journey("fx-review", 2)),
        "journeys-bad/a.json": pretty(journey("fx-run", 4)),
        "journeys-bad/b.json": pretty(journey("fx-run", 1)),
        "journeys-bad/c.json": pretty({"format": "screen-inventory@1", "screens": []}),
    }
    zero = journey("fx-zero", 2)
    zero["steps"][0]["n"] = 0
    out["journeys-bad/d.json"] = pretty(zero)
    return {k: v.encode() for k, v in out.items()}


GLOB_PATTERNS = [
    "view.select_*@1", "view.*@1", "view.*_all@1", "v*w.s*t_*@1", "palette.run_command@1",
    "palette.run_command@2", "report.export_*@2", "report.*@1", "a*a*a@1", "view.select_one*@1",
]
GLOB_NAMES = [
    "view.select_one@1", "view.select_all@1", "view.select_@1", "view.selectall@1", "view.a.b_all@1",
    "palette.run_command@1", "palette.run_commands@1", "report.export_pdf@2", "report.export_pdf@1",
    "aaa@1", "aa@1", "abababa@1", "view.select_one@10",
]


def glob_cases() -> dict:
    rows = []
    for p in GLOB_PATTERNS:
        rx = pattern_regex(p)
        for n in GLOB_NAMES:
            rows.append({"pattern": p, "command": n, "match": rx.match(n) is not None})
    bad = ["view.x", "view.x@0", "view.x@01", "view.x@123456", "*.x@1", "View.x@1", "v**@1", "v.x@a", "@1",
           "v.x@1@2", "v-x@1", "a" * 127 + "@1", "a*b*c*d*e*f*g*h*i*j@1"]
    for b in bad:
        if pattern_ok(b):
            raise SystemExit(f"glob case {b!r} should be malformed")
    return {"format": "pattern-glob-cases@1", "matches": rows, "malformed": bad}


def all_files() -> dict[str, bytes]:
    specs, expect = cases()
    files = {k: v.encode() for k, v in specs.items()}
    files.update(package_files())
    files.update(journey_files())
    files["manifest.json"] = pretty(MANIFEST).encode()
    files["glob-cases.json"] = pretty(glob_cases()).encode()
    files["expected.json"] = pretty({"format": "pattern-check-cases@1", "cases": expect}).encode()
    return files, specs, expect


def verify(specs: dict[str, str], expect: list[dict]) -> list[str]:
    """The reference checker must agree with every hand label."""
    errors = []
    for c in expect:
        err, warn, unchecked = reference(specs, c)
        got = (sorted([list(e) for e in err]), sorted([list(w) for w in warn]), unchecked)
        want = (c["errors"], c["warnings"], c["unchecked"])
        if got != want:
            errors.append(f"reference checker disagrees on {c['root']} ({c['journeys']}): {got} != {want}")
    return errors


def main() -> int:
    files, specs, expect = all_files()
    # The reference checker reads the package and journeys from disk, so write first when
    # generating; --check verifies the files on disk and then the labels against them.
    if "--check" in sys.argv[1:]:
        errors = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file():
                errors.append(f"missing fixture file: {rel}")
            elif p.read_bytes() != want:
                errors.append(f"fixture drift: {rel}")
        for d in ["ok", "extra", "pkg", "journeys", "journeys-bad"]:
            for p in (HERE / d).rglob("*"):
                rel = p.relative_to(HERE).as_posix()
                if p.is_file() and rel not in files:
                    errors.append(f"unexpected file: {rel}")
        if not errors:
            errors = verify(specs, expect)
        for e in errors:
            print(e, file=sys.stderr)
        return 1 if errors else 0
    for rel, content in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(content)
    errors = verify(specs, expect)
    for e in errors:
        print(e, file=sys.stderr)
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
