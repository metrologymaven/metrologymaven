#!/usr/bin/env python3
"""Fixtures of spec node AGENT.SKILLS.STYLE (guide and recipe content linter).

Project-authored synthetic text (CC-BY-4.0); none of it is standards text. Writes one folder
per case next to this script:

    <case>/manifest.json   commands with side-effect classes and task guides (CORE.REG.MAN shape)
    <case>/catalog.json    the curated workflows (fid.agent-catalog v1)
    <case>/skills/<guide-id-with-hyphens>/SKILL.md
    <case>/recipes/<recipe-id>.json
    <case>/prose/*.md      extra Markdown (optional)
    <case>/allow.txt       maintainer allow list (optional)

plus `denylist.txt` (fixture deny list, project-written stems) and `expected.json`, the
hand-labelled findings of every case as [rule, file relative to the case folder, line].
Rules are labelled by hand from the spec; line numbers are found by searching the generated
text for the marked sentence; word counts are counted here with str.split(). Nothing here runs
the Rust code. Run:  python fixtures/agent/skills/style/make_cases.py
"""

import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent

DENYLIST = """\
# Fixture deny list: project-written phrase stems, one entry per line (not standards text).
the rout shall be repeat for each featur until the deviat of the fit is within the stat limit
ever probe shall be qualif befor use and after each chang of the stylus or the temperatur
"""

COMMANDS = [
    ("agent.attach_brief", "W1"),
    ("demo.fit_cylinder", "R1"),
    ("demo.move_axis", "M1"),
    ("demo.save_plan", "W2"),
    ("demo.scan", "R1"),
    ("demo.set_limits", "A0"),
    ("demo.sign_report", "S1"),
    ("journal.promote_protected_branch", "W3"),
    ("journal.request_promote_branch", "W1"),
]


def step(cmd, purpose):
    return {"command": {"id": cmd, "major": 1}, "purpose": purpose}


GUIDES = {
    "demo.measure_bore": {
        "title": "Measure a bore",
        "summary": "Scan the bore and fit a cylinder. Report the fit through the result record, never in prose.",
        "steps": [step("demo.scan", "Scan the bore surface"), step("demo.fit_cylinder", "Fit a cylinder to the scan")],
    },
    "demo.plan_bore": {
        "title": "Plan a bore check",
        "summary": "Draft a bore check on the agent branch and ask a person to promote it.",
        "steps": [
            step("demo.scan", "Scan the bore surface"),
            step("demo.save_plan", "Save the drafted plan"),
            step("agent.attach_brief", "Attach the proposal brief"),
            step("journal.request_promote_branch", "Request promotion of the agent branch"),
        ],
    },
    "demo.plan_no_brief": {
        "title": "Plan without a brief",
        "summary": "A state-changing guide that stops after saving.",
        "steps": [step("demo.scan", "Scan the bore surface"), step("demo.save_plan", "Save the drafted plan")],
    },
    "demo.plan_wrong_order": {
        "title": "Plan with the closing pair reversed",
        "summary": "A state-changing guide whose closing steps are in the wrong order.",
        "steps": [
            step("demo.save_plan", "Save the drafted plan"),
            step("journal.request_promote_branch", "Request promotion of the agent branch"),
            step("agent.attach_brief", "Attach the proposal brief"),
        ],
    },
    "demo.guide_fields": {
        "title": "Guide with bad fields",
        "summary": "Expect a bore of 12.004 mm after the fit.",
        "steps": [step("demo.scan", "Scan, then call demo_sign_report yourself")],
    },
}

SKILL_BODY = {
    "demo.measure_bore": "# Measure a bore\n\nScan the bore, then fit a cylinder to the scan.\n"
    "The fitted size lives in the result record; read it from there.\n",
    "demo.plan_bore": "# Plan a bore check\n\nScan the bore and save the drafted plan on the agent branch.\n"
    "Then attach the proposal brief and request promotion.\n",
    "demo.plan_no_brief": "# Plan without a brief\n\nScan and save.\n",
    "demo.plan_wrong_order": "# Plan with the closing pair reversed\n\nSave, request, attach.\n",
    "demo.guide_fields": "# Guide with bad fields\n\nScan the part.\n",
}

CLEAN_RECIPE = {
    "id": "demo.bore_basic",
    "title": "Basic bore",
    "prompt": "Measure the bore of the fixture part and report through the result record.",
    "commentary": [
        "The agent scans the bore first.",
        "It fits a cylinder; the size is in the result record, not in this text.",
    ],
}

QUOTE_25 = " ".join(["word"] * 25)

CLEAN_NOTES = f"""\
# Notes on the bore workflow

Scan the bore and fit a cylinder. Use step 2 of the guide, version v2.1 of the plan, W2 drafts.

```sh
fid demo fit-cylinder --expect 12.004 mm --tilt 0.5 deg --share 30 %
```

File a request for a human to run `journal_promote_protected_branch` when the plan is ready.

The routine shall be repeated for each feature until the deviation. That is eleven words of a run.

A short quotation is fine: "{QUOTE_25}".

> A short blockquote is fine too.
"""

NUMBERS = """\
# Numbers in prose

After the fit, the bore measures 12.004 mm in this guide.

Tilt is 0.5° and the share is 3 %.

Runout was 5um on the last part.

Repeat step 2 in the next run and pick v2.1 of the guide.
"""

NUMBERS_FENCED = """\
# Numbers in a fenced example

```text
After the fit, the bore measures 12.004 mm in this guide.
```
"""

FORBIDDEN = """\
# Forbidden instructions

Now call `journal_promote_protected_branch` to finish.

File a request for a human to run `journal.promote_protected_branch@1` after review.

Run demo.move_axis@1 to home the probe.

```sh
fid call demo_set_limits
```
"""

STANDARDS = f"""\
# Possible standards text

The routine shall be repeated for each feature until the deviation of it settles.

Every probe shall be qualified before use and after each change of the stylus, says a note.

The text reads "{" ".join(["word"] * 24)}
word word" and continues.

> {QUOTE_25}
> two more
"""


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(text.encode("utf-8"))


def write_json(path, obj):
    write(path, json.dumps(obj, indent=2, ensure_ascii=False) + "\n")


def skill_dir(gid):
    return gid.replace(".", "-").replace("_", "-")


def skill_text(gid, body):
    return f"---\nname: {skill_dir(gid)}\ndescription: {GUIDES[gid]['title']}\nmanifest_digest: sha256:{'0' * 64}\n---\n{body}"


def line_of(text, marker):
    hits = [i + 1 for i, l in enumerate(text.split("\n")) if marker in l]
    assert len(hits) == 1, (marker, hits)
    return hits[0]


def make_case(name, guides, prose=None, allow=None, skill_bodies=None):
    root = HERE / name
    if root.exists():
        shutil.rmtree(root)
    manifest = {
        "commands": [{"id": c, "major": 1, "side_effect": k} for c, k in COMMANDS],
        "format": 1,
        "task_guides": [dict(id=g, profile="inspect", **GUIDES[g]) for g in sorted(GUIDES)],
    }
    write_json(root / "manifest.json", manifest)
    rows = [
        {"profile": "inspect", "workflow_id": g, "title": GUIDES[g]["title"], "guide_id": g, "recipes": []}
        for g in guides
    ]
    write_json(root / "catalog.json", {"format": "fid.agent-catalog", "version": 1, "workflows": rows})
    bodies = skill_bodies or {}
    for g in guides:
        write(root / "skills" / skill_dir(g) / "SKILL.md", skill_text(g, bodies.get(g, SKILL_BODY[g])))
    for fname, text in (prose or {}).items():
        write(root / "prose" / fname, text)
    if allow is not None:
        write(root / "allow.txt", allow)
    return root


def main():
    write(HERE / "denylist.txt", DENYLIST)
    expected = {}

    # AC-1: guides, Skills, recipes and notes that follow every rule.
    root = make_case("clean", ["demo.measure_bore", "demo.plan_bore"], prose={"notes.md": CLEAN_NOTES})
    write_json(root / "recipes" / "demo.bore_basic.json", CLEAN_RECIPE)
    cat = json.loads((root / "catalog.json").read_text("utf-8"))
    cat["workflows"][0]["recipes"] = ["demo.bore_basic"]
    write_json(root / "catalog.json", cat)
    assert len(QUOTE_25.split()) == 25
    expected["clean"] = []

    # AC-2: numbers with units in prose; the same text in a fence passes.
    make_case("numbers", ["demo.measure_bore"], prose={"numbers.md": NUMBERS})
    expected["numbers"] = [
        ["result_like_number", "prose/numbers.md", line_of(NUMBERS, "12.004 mm")],
        ["result_like_number", "prose/numbers.md", line_of(NUMBERS, "0.5°")],
        ["result_like_number", "prose/numbers.md", line_of(NUMBERS, "0.5°")],
        ["result_like_number", "prose/numbers.md", line_of(NUMBERS, "5um")],
    ]
    make_case("numbers-fenced", ["demo.measure_bore"], prose={"numbers.md": NUMBERS_FENCED})
    expected["numbers-fenced"] = []

    # Allow list: the maintainer allows every numbers finding by rule, line and file.
    allow = "# rule line file\n" + "".join(
        f"{r} {n} {f}\n" for r, f, n in sorted({tuple(x) for x in expected["numbers"]})
    )
    make_case("allow", ["demo.measure_bore"], prose={"numbers.md": NUMBERS}, allow=allow)
    expected["allow"] = []

    # AC-3: a W2 step without the closing brief and promotion request, and the pair reversed.
    make_case("proposal", ["demo.plan_bore", "demo.plan_no_brief", "demo.plan_wrong_order"])
    expected["proposal"] = [
        ["missing_proposal_step", "manifest.json#task_guides/demo.plan_no_brief", 2],
        ["missing_proposal_step", "manifest.json#task_guides/demo.plan_wrong_order", 3],
    ]

    # AC-4: forbidden commands named in prose, recipe commentary and a fenced example.
    bad_recipe = dict(CLEAN_RECIPE, commentary=["Then sign it with demo_sign_report."])
    root = make_case("forbidden", ["demo.measure_bore"], prose={"forbidden.md": FORBIDDEN})
    write_json(root / "recipes" / "demo.bore_basic.json", bad_recipe)
    cat = json.loads((root / "catalog.json").read_text("utf-8"))
    cat["workflows"][0]["recipes"] = ["demo.bore_basic"]
    write_json(root / "catalog.json", cat)
    expected["forbidden"] = [
        ["forbidden_instruction", "prose/forbidden.md", line_of(FORBIDDEN, "Now call")],
        ["forbidden_instruction", "prose/forbidden.md", line_of(FORBIDDEN, "demo.move_axis@1")],
        ["forbidden_instruction", "prose/forbidden.md", line_of(FORBIDDEN, "demo_set_limits")],
        ["forbidden_instruction", "recipes/demo.bore_basic.json#commentary/0", 1],
    ]

    # Guide fields from the manifest: a number in the summary, a forbidden tool in a purpose.
    make_case("guide-fields", ["demo.guide_fields"])
    expected["guide-fields"] = [
        ["forbidden_instruction", "manifest.json#task_guides/demo.guide_fields/steps/0/purpose", 1],
        ["result_like_number", "manifest.json#task_guides/demo.guide_fields/summary", 1],
    ]

    # AC-5: a Skill body of 1600 words fails; one of exactly 1500 passes.
    def words(n):
        ws = ["scan", "the", "bore", "then", "fit", "a", "cylinder", "and", "read", "on"] * (n // 10)
        return "\n".join(" ".join(ws[i : i + 10]) for i in range(0, n, 10)) + "\n"

    b1600, b1500 = words(1600), words(1500)
    assert len(b1600.split()) == 1600 and len(b1500.split()) == 1500
    make_case(
        "budget",
        ["demo.measure_bore", "demo.plan_bore"],
        skill_bodies={"demo.measure_bore": b1600, "demo.plan_bore": b1500},
    )
    front_matter_lines = 5
    expected["budget"] = [["over_budget", "skills/demo-measure-bore/SKILL.md", front_matter_lines + 1]]

    # Deny-list runs of 12 words and quotations over 25 words.
    make_case("standards", ["demo.measure_bore"], prose={"standards.md": STANDARDS})
    q = STANDARDS.split('"')[1]
    assert len(q.split()) == 26, len(q.split())
    expected["standards"] = [
        ["possible_standards_text", "prose/standards.md", line_of(STANDARDS, "The routine")],
        ["possible_standards_text", "prose/standards.md", line_of(STANDARDS, "Every probe")],
        ["possible_standards_text", "prose/standards.md", line_of(STANDARDS, "The text reads")],
        ["possible_standards_text", "prose/standards.md", line_of(STANDARDS, "> word")],
    ]

    for k in expected:
        expected[k].sort(key=lambda x: (x[1], x[2], x[0]))
    write_json(HERE / "expected.json", expected)


if __name__ == "__main__":
    main()
