"""Writes the fixtures of spec node OUTR.DOCS.REF (CC-BY-4.0) and an independent rendering of
the expected reference pages.

Every word here is project-authored and synthetic. The registry manifest export
(CORE.REG.MAN) and the docs-ref-v1 emitter (AUTO.SURF.DOCS) are not built yet, so the manifest
`min/min-classes.json` and the records under `min/records/` are hand-written stand-ins that
follow the shapes those two leaves specify: one command per side-effect class of CONSTITUTION
section 3, plus one `human_only` W2 command. Each record's MCP reachability is hand-labelled
from the rules in AUTO.SURF.DOCS (Design: Algorithm).

The expected pages under `expected/` and `expected-python/` are rendered by THIS script from
the page layout written in OUTR.DOCS.REF (Design: Page layout) and in the module docs of
crates/fid-outreach/src/ref_pages.rs. They are a second, independent implementation of that
layout; crates/fid-outreach/tests/ref_pages.rs asserts that the Rust renderer produces the same
bytes. Nothing here was produced by running the Rust code.

Layout:
  min/min-classes.json             manifest (12 command majors)
  min/surfaces.toml                release ships cli and mcp
  min/surfaces-python.toml         release ships cli, mcp and python
  min/records/<id>@<major>.json    one docs-ref-v1 record per command major
  missing-record/min-classes.json  the manifest plus demo.orphan@1, which has no record
  missing-record/records/          the 12 records, naming that manifest's digest
  stale/records/                   the 12 records, naming a digest that is not the manifest's
  expected/v0.1.0/ref/<cli|mcp|python>/<id>@<major>.html   pages for min/surfaces.toml
  expected-python/python/<id>@<major>.html                 Python pages for surfaces-python.toml

Run from the repository root:
  python fixtures/outr/docs/ref/make_fixtures.py          (write)
  python fixtures/outr/docs/ref/make_fixtures.py --check  (exit 1 if any file differs)
"""

import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
GENERATED_DIRS = ["min", "missing-record", "stale", "expected", "expected-python"]


def canon(value):
    """RFC 8785 for the value shapes used here (ASCII strings, small integers, bools, null)."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


# (id, major, side_effect, determinism, human_only, mcp projection, mcp available, reason)
# Reasons follow AUTO.SURF.DOCS: projection none -> class_never_on_mcp; request_only ->
# request_only with the tool name; human_only overrides named or exposed.
COMMANDS = [
    ("demo.read_item", 1, "R0", "D0", False, "exposed", True, None),
    ("demo.compute_sum", 2, "R1", "D0", False, "exposed", True, None),
    ("demo.edit_draft", 1, "W1", "D0", False, "named", True, None),
    ("demo.edit_doc", 1, "W2", "D0", False, "named", True, None),
    ("demo.adopt_doc", 1, "W2", "D0", True, "request_only", True, "human_only"),
    ("demo.release_record", 1, "W3", "D0", False, "request_only", True, "request_only"),
    ("demo.export_report", 1, "X1", "N", False, "request_only", True, "request_only"),
    ("demo.sim_step", 1, "I1", "D1", False, "exposed", True, None),
    ("demo.probe_power", 1, "D1", "N", False, "request_only", True, "request_only"),
    ("demo.move_axis", 1, "M1", "N", False, "none", False, "class_never_on_mcp"),
    ("demo.sign_record", 1, "S1", "D0", False, "none", False, "class_never_on_mcp"),
    ("demo.set_limits", 1, "A0", "D0", False, "none", False, "class_never_on_mcp"),
]


def params_for(cid):
    if cid == "demo.compute_sum":
        return [
            {"name": "a", "ty": "f64", "quantity_kind": "length", "unit": "mm", "default": None},
            {"name": "b", "ty": "f64", "quantity_kind": "length", "unit": "mm", "default": 0},
        ]
    if cid == "demo.move_axis":
        return [
            {"name": "axis", "ty": "String", "quantity_kind": None, "unit": None, "default": "x"},
            {"name": "dry_run", "ty": "bool", "quantity_kind": None, "unit": None, "default": True},
        ]
    if cid == "demo.read_item":
        return []
    return [{"name": "target", "ty": "Handle", "quantity_kind": None, "unit": None, "default": None}]


def help_for(cid):
    verb = cid.split(".", 1)[1].replace("_", " ")
    text = "Synthetic fixture command: " + verb + ".\n\nUsage notes for <" + cid + ">."
    if cid == "demo.read_item":
        # Characters that must be HTML-escaped, kept verbatim in the record.
        text += "\nCompare a < b & c > d, \"quoted\" and 'single'."
    return text


def manifest_bytes(extra=()):
    commands = []
    for cid, major, se, det, human, _proj, _avail, _reason in list(COMMANDS) + list(extra):
        entry = {"id": cid, "major": major, "side_effect": se, "determinism": det, "human_only": human}
        entry["definition_digest"] = sha256(canon(entry))
        commands.append(entry)
    commands.sort(key=lambda c: (c["id"], c["major"]))
    doc = {
        "format": 1,
        "quantity_catalogue_digest": sha256(b"synthetic quantity catalogue for OUTR.DOCS.REF fixtures"),
        "commands": commands,
    }
    return canon(doc)


def record(cmd, manifest_digest):
    cid, major, se, det, human, proj, avail, reason = cmd
    domain, verb = cid.split(".", 1)
    mcp = {"available": avail, "projection": proj}
    if avail:
        mcp["name"] = cid.replace(".", "_")
    if reason is not None:
        mcp["reason"] = reason
    rec = {
        "schema": "docs-ref-v1",
        "manifest_digest": manifest_digest,
        "command_digest": sha256(canon({"id": cid, "major": major})),
        "command_id": cid,
        "major": major,
        "summary": "Synthetic " + se + " fixture command " + cid + ".",
        "help": help_for(cid),
        "params": params_for(cid),
        "result": "Handle" if se.startswith("W") else {"type": "Report", "fields": ["ok"]},
        "side_effect": se,
        "determinism": det,
        "human_only": human,
        "long_running": False,
        "dry_run": se == "M1",
        "surfaces": {
            "cli": {"available": True, "name": "fid " + domain + " " + verb.replace("_", "-")},
            "mcp": mcp,
            "python": {"available": True, "name": "fiducial." + domain + "." + verb},
        },
    }
    return rec


# ------------------------------------------------------------------ independent page layout

REASON_WORDS = {
    "class_never_on_mcp": "never on MCP for this class",
    "request_only": "request-only",
    "human_only": "human-only",
}
SURFACE_LABEL = {"cli": "CLI", "mcp": "MCP tool", "python": "Python"}
SURFACE_WORD = {"cli": "the CLI", "mcp": "MCP", "python": "Python"}


def esc(s):
    return (
        s.replace("&", "&amp;")
        .replace("<", "&lt;")
        .replace(">", "&gt;")
        .replace('"', "&quot;")
        .replace("'", "&#39;")
    )


def text(v):
    if v is None:
        return ""
    if isinstance(v, str):
        return v
    return canon(v).decode("utf-8")


def params_html(rec):
    out = "<h2>Parameters</h2>\n<table class=\"params\">\n"
    out += "<thead><tr><th>Name</th><th>Type</th><th>Quantity kind</th><th>Unit</th><th>Default</th></tr></thead>\n"
    out += "<tbody>\n"
    for p in rec["params"]:
        out += "<tr><td><code>" + esc(p["name"]) + "</code></td><td>" + esc(p["ty"]) + "</td>"
        out += "<td>" + esc(text(p.get("quantity_kind"))) + "</td><td>" + esc(text(p.get("unit"))) + "</td>"
        out += "<td>" + esc(text(p.get("default"))) + "</td></tr>\n"
    out += "</tbody>\n</table>\n"
    out += "<h2>Result</h2>\n<p><code>" + esc(text(rec["result"])) + "</code></p>\n"
    return out


def page(rec, surface, ships_python):
    cmd = rec["command_id"] + "@" + str(rec["major"])
    mcp = rec["surfaces"]["mcp"]
    out = "<article class=\"ref-page ref-" + surface + "\">\n"
    out += "<h1><code>" + esc(cmd) + "</code></h1>\n"
    out += "<p class=\"summary\">" + esc(rec["summary"]) + "</p>\n"
    out += "<dl class=\"ref-facts\">\n"
    out += "<dt>Command</dt><dd><code>" + esc(cmd) + "</code></dd>\n"
    out += "<dt>Side-effect class</dt><dd>" + esc(rec["side_effect"]) + "</dd>\n"
    out += "<dt>Determinism</dt><dd>" + esc(rec["determinism"]) + "</dd>\n"
    out += "<dt>Human only</dt><dd>" + ("true" if rec["human_only"] else "false") + "</dd>\n"
    out += "<dt>MCP</dt><dd>" + esc(text(mcp.get("projection"))) + "</dd>\n"
    out += "</dl>\n"
    out += "<pre class=\"help\">" + esc(rec["help"]) + "</pre>\n"
    out += "<h2>" + SURFACE_LABEL[surface] + "</h2>\n"
    s = rec["surfaces"][surface]
    if surface == "python" and not ships_python:
        out += "<p class=\"unavailable\">Python bindings are not part of this release.</p>\n"
    elif s["available"]:
        out += "<p><code>" + esc(s["name"]) + "</code></p>\n"
        if s.get("reason") is not None:
            out += "<p class=\"reason\">Reason: " + REASON_WORDS[s["reason"]] + "</p>\n"
        out += params_html(rec)
    else:
        out += "<p class=\"unavailable\">This command is not available on " + SURFACE_WORD[surface] + ".</p>\n"
        if s.get("reason") is not None:
            out += "<p class=\"reason\">Reason: " + REASON_WORDS[s["reason"]] + "</p>\n"
    out += "</article>\n"
    return out.encode("utf-8")


# ------------------------------------------------------------------ assembly

def build():
    files = {}
    man = manifest_bytes()
    digest = sha256(man)
    files["min/min-classes.json"] = man
    files["min/surfaces.toml"] = b"# Synthetic fixture: the release ships the CLI and MCP surfaces.\nsurfaces = [\"cli\", \"mcp\"]\n"
    files["min/surfaces-python.toml"] = (
        b"# Synthetic fixture: the release also ships Python bindings.\nsurfaces = [\"cli\", \"mcp\", \"python\"]\n"
    )
    recs = [record(c, digest) for c in COMMANDS]
    for rec in recs:
        name = rec["command_id"] + "@" + str(rec["major"])
        files["min/records/" + name + ".json"] = canon(rec)
        for surface in ("cli", "mcp", "python"):
            files["expected/v0.1.0/ref/" + surface + "/" + name + ".html"] = page(rec, surface, False)
        files["expected-python/python/" + name + ".html"] = page(rec, "python", True)

    orphan = ("demo.orphan", 1, "R0", "D0", False, "exposed", True, None)
    man2 = manifest_bytes([orphan])
    files["missing-record/min-classes.json"] = man2
    for c in COMMANDS:
        rec = record(c, sha256(man2))
        files["missing-record/records/" + rec["command_id"] + "@" + str(rec["major"]) + ".json"] = canon(rec)

    stale_digest = sha256(b"a different manifest")
    for c in COMMANDS:
        rec = record(c, stale_digest)
        files["stale/records/" + rec["command_id"] + "@" + str(rec["major"]) + ".json"] = canon(rec)
    return files


def on_disk():
    found = {}
    for top in GENERATED_DIRS:
        root = os.path.join(HERE, top)
        for dirpath, _dirs, names in os.walk(root):
            for n in names:
                full = os.path.join(dirpath, n)
                rel = os.path.relpath(full, HERE).replace(os.sep, "/")
                with open(full, "rb") as f:
                    found[rel] = f.read()
    return found


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        disk = on_disk()
        bad = sorted(set(files) ^ set(disk)) + sorted(k for k in files if k in disk and files[k] != disk[k])
        for b in bad:
            print("stale fixture: " + b)
        return 1 if bad else 0
    for rel, data in files.items():
        path = os.path.join(HERE, *rel.split("/"))
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(data)
    print("wrote %d files" % len(files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
