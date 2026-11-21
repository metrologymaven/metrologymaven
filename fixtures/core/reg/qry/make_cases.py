#!/usr/bin/env python3
"""Generate the fixtures of spec node CORE.REG.QRY (CC-BY-4.0 project data). Stdlib only.

An independent reference: every expected result below is computed here, from the rules of
spec/core/reg/qry/NODE.md and the pinned choices listed next, with Python's own json,
hashlib and a Levenshtein written here. The Rust crate is never run to produce them.

Pinned rules (also stated in the module docs of crates/fid-registry/src/query.rs):

- The MCP projection of a command is the stricter of its declared projection (`human_only`
  clamps `exposed` to `request_only`, D-031) and the ceiling of its class (R0 R1 I1 W1 W2
  `exposed`; W3 X1 D1 `request_only`; M1 S1 A0 `never`), and `never` when `mcp` is not in
  its exposure (CORE.BUS.POL.PROJ, `mcp_projection`).
- Audience `agent` (an MCP channel or an agent principal) sees only commands whose MCP
  projection is not `never` (D-035); to it every other command does not exist: not listed,
  `not_found` when described, never suggested. Audience `person` sees every command.
- A summary is {id, major, title, lifecycle (the state), side_effect, mcp_projection (the
  MCP projection above)}; list results are sorted by id bytes, then major.
- `list_commands` filters: `domain` is the ID's text before its first dot; `side_effect`
  and `lifecycle` match exactly; all given filters must match. Majors in state `hidden`
  appear only with `include_hidden: true`.
- `describe_command` without `major` takes the highest `active` major; none is
  `not_found`. For an agent, `lifecycle.replaced_by` is dropped when it names a command the
  agent cannot see; `definition_digest` is always the digest of the full definition. The definition is the manifest's normal form without `definition_digest`;
  the digest is SHA-256 of its RFC 8785 bytes.
- `not_found` suggests up to five distinct IDs the audience can see, closest first by
  Levenshtein distance over Unicode scalar values, ties by ID bytes.
- `diff_manifests` gives one entry per changed (id, major), sorted by id then major:
  `added` (additive), `removed` (breaking), `lifecycle` (breaking only when the new state is
  `removed`), or `changed` (breaking when any classified change is breaking); `fields` is
  the sorted list of changed JSON pointers (`/lifecycle` included), empty for added and
  removed. The result also carries `a_manifest_digest` and `b_manifest_digest`, SHA-256 of
  the RFC 8785 form of each manifest. The classified changes are taken from the hand-labelled
  ../manifest/v1_v2.diff.json (fixtures/core/reg/ver/compat/make_fixtures.py).

Outputs:
  decls.json          query test declarations (hidden, deprecated, removed, human_only,
                      never-projected and MCP-less commands), derived from ../decls/valid.json
  cases.json          list/describe cases over decls.json ("qry") and ../decls/valid.json
                      ("valid"), each with its expected result or error
  projection.json     the MCP projection of every class x projection x human_only x
                      exposure combination
  audience_{a,b}.manifest.json, audience.changes.json
                      two manifests and the expected diff for a person and for an agent
                      (an agent sees a change only when the command is not `never` on MCP in
                      every manifest that holds it)
  v1_v2.changes.json  the expected `registry.diff_manifests@1` result for
                      ../manifest/v1.json -> v2.json (AC-3)

Run:    python fixtures/core/reg/qry/make_cases.py
Check:  python fixtures/core/reg/qry/make_cases.py --check
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
VALID = HERE.parent / "decls" / "valid.json"
COMPAT_DIFF = HERE.parent / "manifest" / "v1_v2.diff.json"
SURFACES = ["cli", "mcp", "python", "gui", "control"]
ORDER = {"never": 0, "request_only": 1, "exposed": 2}
CEILING = {"R0": "exposed", "R1": "exposed", "I1": "exposed", "W1": "exposed", "W2": "exposed",
           "W3": "request_only", "X1": "request_only", "D1": "request_only",
           "M1": "never", "S1": "never", "A0": "never"}


def jcs(v):
    """RFC 8785 for the values these fixtures hold (integers only, no floats)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, float):
        if v != v or v != int(v) or abs(v) >= 1e21:
            raise ValueError("fixture numbers must be integral")
        return str(int(v))
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(json.dumps(k, ensure_ascii=False) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def normal(decl):
    d = copy.deepcopy(decl)
    d.setdefault("human_only", False)
    d.setdefault("network", "none")
    d.setdefault("cli_bare", False)
    d["exposure"] = sorted(d["exposure"], key=SURFACES.index)
    return d


def projection(d):
    declared = d["mcp_projection"]
    if d.get("human_only", False) and declared == "exposed":
        declared = "request_only"
    ceiling = CEILING[d["side_effect"]]
    p = declared if ORDER[declared] < ORDER[ceiling] else ceiling
    return p if "mcp" in d["exposure"] else "never"


def levenshtein(a, b):
    prev = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        cur = [i]
        for j, cb in enumerate(b, 1):
            cur.append(min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (ca != cb)))
        prev = cur
    return prev[-1]


# ------------------------------------------------------------------ declarations

def qry_decls(valid):
    by_id = {(d["id"], d["major"]): d for d in valid}

    def take(id_, major=1, **over):
        d = copy.deepcopy(by_id[(id_, major)])
        d.update(over)
        return d

    def renamed(src, id_, major=1, **over):
        d = take(src, **over)
        d["id"], d["major"] = id_, major
        d["title"] = f"Query fixture {id_}@{major}"
        return d

    return [
        # W2 human_only exposed on MCP: an agent sees it as request_only.
        take("align.run_bestfit", exposure=["mcp", "cli"]),
        # A hidden later major, listed out of surface order.
        renamed("align.run_bestfit", "align.run_bestfit", 2, human_only=False, exposure=["mcp", "cli"],
                lifecycle={"state": "hidden", "since": "2026-06-01"}),
        # W1 without the MCP surface: never on MCP although its class allows exposed.
        renamed("report.set_title", "align.set_datum", exposure=["cli"]),
        renamed("gdt.evaluate_flatness", "gdt.evaluate_flatness", 1, exposure=["cli", "mcp"],
                lifecycle={"state": "deprecated", "since": "2026-09-01", "replaced_by": "gdt.evaluate_flatness@2"}),
        renamed("gdt.evaluate_flatness", "gdt.evaluate_flatness", 2, exposure=["python", "cli", "mcp"]),
        take("record.release_fai", exposure=["cli", "mcp"]),
        # A0 human_only: never on MCP.
        renamed("settings.set_admin_value", "sign.set_profile", human_only=True, exposure=["cli", "mcp"]),
        take("cmm.move_to", exposure=["cli", "mcp"]),
        renamed("project.list_features", "project.list_features", 1, exposure=["cli", "mcp"],
                lifecycle={"state": "removed", "since": "2026-10-01"}),
        take("sim.read_probe", exposure=["gui", "cli", "mcp"], human_only=False),
        # Near neighbours for the suggestion tie-break.
        renamed("sim.read_probe", "sim.read_prob", exposure=["cli", "mcp"]),
        renamed("sim.read_probe", "sim.read_probes", exposure=["cli", "mcp"]),
        # A visible deprecated major whose successor an agent cannot see (hidden and not on
        # MCP): an agent's describe drops replaced_by (#394 review).
        renamed("sim.read_probe", "x.tool", 1, exposure=["cli", "mcp"],
                lifecycle={"state": "deprecated", "since": "2026-09-01", "replaced_by": "x.tool@2"}),
        renamed("sim.read_probe", "x.tool", 2, exposure=["cli"],
                lifecycle={"state": "hidden", "since": "2026-09-01"}),
        # The same with a successor that is hidden but on MCP: an agent may list it with
        # include_hidden, so replaced_by stays.
        renamed("sim.read_probe", "y.tool", 1, exposure=["cli", "mcp"],
                lifecycle={"state": "deprecated", "since": "2026-09-01", "replaced_by": "y.tool@2"}),
        renamed("sim.read_probe", "y.tool", 2, exposure=["cli", "mcp"],
                lifecycle={"state": "hidden", "since": "2026-09-01"}),
    ]


# ------------------------------------------------------------------ queries

def state(d):
    return d["lifecycle"]["state"]


def visible(decls, audience):
    return [d for d in decls if audience == "person" or projection(d) != "never"]


def summary(d):
    return {"id": d["id"], "major": d["major"], "title": d["title"], "lifecycle": state(d),
            "side_effect": d["side_effect"], "mcp_projection": projection(d)}


def key(d):
    return (d["id"].encode("utf-8"), d["major"])


def list_commands(decls, audience, params):
    out = []
    for d in sorted(visible(decls, audience), key=key):
        if state(d) == "hidden" and not params.get("include_hidden", False):
            continue
        if "domain" in params and d["id"].split(".", 1)[0] != params["domain"]:
            continue
        if "side_effect" in params and d["side_effect"] != params["side_effect"]:
            continue
        if "lifecycle" in params and state(d) != params["lifecycle"]:
            continue
        out.append(summary(d))
    return {"ok": {"commands": out}}


def suggestions(decls, audience, id_):
    ids = sorted({d["id"] for d in visible(decls, audience)}, key=lambda s: s.encode("utf-8"))
    ranked = sorted(ids, key=lambda s: (levenshtein(id_, s), s.encode("utf-8")))
    return ranked[:5]


def describe_command(decls, audience, params):
    id_ = params["id"]
    seen = [d for d in visible(decls, audience) if d["id"] == id_]
    if "major" in params:
        hit = [d for d in seen if d["major"] == params["major"]]
    else:
        active = sorted((d for d in seen if state(d) == "active"), key=lambda d: d["major"])
        hit = active[-1:]
    if not hit:
        return {"error": {"code": "not_found", "suggestions": suggestions(decls, audience, id_)}}
    body = normal(hit[0])
    digest = sha(jcs(body))
    successor = body["lifecycle"].get("replaced_by")
    if audience == "agent" and successor is not None:
        sid, _, smajor = successor.rpartition("@")
        if not any(d["id"] == sid and str(d["major"]) == smajor for d in visible(decls, audience)):
            body = copy.deepcopy(body)
            del body["lifecycle"]["replaced_by"]
    # The digest is always that of the full definition in the manifest.
    return {"ok": {"definition": body, "definition_digest": digest}}


def bad(name, source, audience, tool, params):
    return {"name": name, "source": source, "audience": audience, "tool": tool, "params": params,
            "expect": {"error": {"code": "bad_params"}}}


def cases(sources):
    out = []

    def add(name, source, audience, tool, params):
        fn = list_commands if tool == "list" else describe_command
        out.append({"name": name, "source": source, "audience": audience, "tool": tool, "params": params,
                    "expect": fn(sources[source], audience, params)})

    add("ac1_valid_all", "valid", "person", "list", {})
    add("ac2_valid_w2", "valid", "person", "list", {"side_effect": "W2"})
    add("valid_agent_sees_nothing", "valid", "agent", "list", {})
    add("ac4_valid_bestfi", "valid", "person", "describe", {"id": "align.run_bestfi", "major": 1})
    add("qry_person_all", "qry", "person", "list", {})
    add("qry_person_hidden", "qry", "person", "list", {"include_hidden": True})
    add("qry_agent_all", "qry", "agent", "list", {})
    add("qry_agent_hidden", "qry", "agent", "list", {"include_hidden": True})
    add("qry_domain_align", "qry", "person", "list", {"domain": "align"})
    add("qry_domain_align_w2_hidden", "qry", "person", "list",
        {"domain": "align", "side_effect": "W2", "include_hidden": True})
    add("qry_deprecated", "qry", "person", "list", {"lifecycle": "deprecated"})
    add("qry_hidden_without_flag", "qry", "person", "list", {"lifecycle": "hidden"})
    add("qry_hidden_flag_false", "qry", "person", "list", {"lifecycle": "hidden", "include_hidden": False})
    add("qry_hidden_with_flag_true", "qry", "person", "list", {"lifecycle": "hidden", "include_hidden": True})
    add("qry_agent_never_domain", "qry", "agent", "list", {"domain": "sign"})
    add("qry_agent_a0_filter", "qry", "agent", "list", {"side_effect": "A0", "include_hidden": True})
    add("qry_person_a0_filter", "qry", "person", "list", {"side_effect": "A0"})
    add("qry_no_match", "qry", "person", "list", {"domain": "zzz"})
    add("describe_major", "qry", "person", "describe", {"id": "align.run_bestfit", "major": 1})
    add("describe_latest_active", "qry", "person", "describe", {"id": "gdt.evaluate_flatness"})
    add("describe_skips_hidden_major", "qry", "person", "describe", {"id": "align.run_bestfit"})
    add("describe_hidden_by_major", "qry", "person", "describe", {"id": "align.run_bestfit", "major": 2})
    add("describe_deprecated_by_major", "qry", "person", "describe", {"id": "gdt.evaluate_flatness", "major": 1})
    add("describe_removed_only", "qry", "person", "describe", {"id": "project.list_features"})
    add("describe_unknown_major", "qry", "person", "describe", {"id": "align.run_bestfit", "major": 3})
    add("describe_ties", "qry", "person", "describe", {"id": "sim.read_proba"})
    add("agent_describe_human_only_request_only", "qry", "agent", "describe", {"id": "align.run_bestfit"})
    add("agent_describe_never_a0", "qry", "agent", "describe", {"id": "sign.set_profile", "major": 1})
    add("agent_describe_never_no_mcp", "qry", "agent", "describe", {"id": "align.set_datum"})
    add("agent_describe_never_m1", "qry", "agent", "describe", {"id": "cmm.move_to"})
    add("person_describe_no_mcp", "qry", "person", "describe", {"id": "align.set_datum"})
    add("agent_describe_typo_near_never", "qry", "agent", "describe", {"id": "sign.set_profil"})
    add("agent_describe_drops_invisible_successor", "qry", "agent", "describe", {"id": "x.tool", "major": 1})
    add("person_describe_keeps_successor", "qry", "person", "describe", {"id": "x.tool", "major": 1})
    add("agent_describe_keeps_visible_successor", "qry", "agent", "describe", {"id": "y.tool", "major": 1})
    add("agent_describe_invisible_successor_itself", "qry", "agent", "describe", {"id": "x.tool", "major": 2})
    add("describe_non_ascii", "qry", "person", "describe", {"id": "sim.réad_probe"})

    out.append(bad("list_unknown_key", "qry", "person", "list", {"domains": "align"}))
    out.append(bad("list_domain_type", "qry", "person", "list", {"domain": 5}))
    out.append(bad("list_domain_empty", "qry", "person", "list", {"domain": ""}))
    out.append(bad("list_domain_long", "qry", "person", "list", {"domain": "a" * 65}))
    out.append(bad("list_bad_class", "qry", "person", "list", {"side_effect": "W9"}))
    out.append(bad("list_bad_lifecycle", "qry", "person", "list", {"lifecycle": "gone"}))
    out.append(bad("list_hidden_type", "qry", "agent", "list", {"include_hidden": "yes"}))
    out.append(bad("list_not_object", "qry", "person", "list", ["domain"]))
    out.append(bad("describe_missing_id", "qry", "person", "describe", {"major": 1}))
    out.append(bad("describe_id_type", "qry", "person", "describe", {"id": 7}))
    out.append(bad("describe_id_long", "qry", "person", "describe", {"id": "a" * 257}))
    out.append(bad("describe_major_negative", "qry", "person", "describe", {"id": "cmm.move_to", "major": -1}))
    out.append(bad("describe_major_big", "qry", "person", "describe", {"id": "cmm.move_to", "major": 4294967296}))
    out.append(bad("describe_unknown_key", "qry", "agent", "describe", {"id": "cmm.move_to", "verbose": True}))
    return out


# ------------------------------------------------------------------ diff

def expected_changes():
    """AC-3 from the hand-labelled compat diff, grouped per (id, major)."""
    src = json.loads(COMPAT_DIFF.read_text(encoding="utf-8"))
    acc = {}

    def entry(cmd, major, kind):
        return acc.setdefault((cmd.encode("utf-8"), major), {"id": cmd, "major": major, "kind": kind,
                                                             "compat": "additive", "fields": set()})

    for a in src["added"]:
        entry(a["command"], a["major"], "added")
    for r in src["removed"]:
        entry(r["command"], r["major"], "removed")["compat"] = "breaking"
    for lc in src["lifecycle_changed"]:
        e = entry(lc["command"], lc["major"], "lifecycle")
        e["fields"].add(lc["pointer"])
        if lc["new_state"] == "removed":
            e["compat"] = "breaking"
    for c in src["changed"]:
        e = entry(c["command"], c["major"], "changed")
        e["kind"] = "changed"
        e["fields"].add(c["pointer"])
        if c["kind"] == "breaking":
            e["compat"] = "breaking"
    out = []
    for k in sorted(acc):
        e = acc[k]
        e["fields"] = sorted(e["fields"], key=lambda s: s.encode("utf-8"))
        out.append(e)
    # Hand check of AC-3: exactly the five entries the leaf names.
    assert [(e["id"], e["kind"], e["compat"]) for e in out] == [
        ("demo.export", "added", "additive"),
        ("demo.legacy", "removed", "breaking"),
        ("demo.measure", "changed", "additive"),
        ("demo.probe", "lifecycle", "additive"),
        ("demo.scan", "changed", "breaking"),
    ], out
    digests = {}
    for side in ("v1", "v2"):
        doc = json.loads((HERE.parent / "manifest" / f"{side}.json").read_text(encoding="utf-8"))
        digests[side] = sha(jcs(doc))
    return {"a_manifest_digest": digests["v1"], "b_manifest_digest": digests["v2"], "changes": out}


# ------------------------------------------------------------------ audience-filtered diff

def manifest_text(decls):
    """A CORE.REG.MAN manifest (canonical bytes) of `decls`."""
    catalogue = (HERE.parent / "def" / "schema" / "catalogue" / "geo_frame_units.sha256").read_text(
        encoding="utf-8").strip()
    commands = []
    for d in sorted(decls, key=key):
        body = normal(d)
        body["definition_digest"] = sha(jcs(body))
        commands.append(body)
    return jcs({"format": 1, "quantity_catalogue_digest": catalogue, "commands": commands})


def audience_diff(qry):
    """Two manifests of the qry declarations and the expected diff per audience.

    Hand-labelled changes from a to b:
      align.set_datum@1  title changed     (never on MCP)    changed additive ["/title"]
      cmm.move_home@1    added             (M1, never)       added additive
      cmm.move_to@1      removed           (M1, never)       removed breaking
      sim.read_prob@1    mcp dropped from exposure, so never on MCP in b only
                                                             changed breaking ["/exposure"]
      sim.read_probe@1   title changed     (exposed)         changed additive ["/title"]
    An agent sees only the last: every other command is never-projected on at least one side.
    """
    a = copy.deepcopy(qry)
    b = []
    for d in copy.deepcopy(qry):
        if (d["id"], d["major"]) == ("cmm.move_to", 1):
            moved = copy.deepcopy(d)
            moved["id"] = "cmm.move_home"
            moved["title"] = "Query fixture cmm.move_home@1"
            b.append(moved)
            continue
        if (d["id"], d["major"]) in (("align.set_datum", 1), ("sim.read_probe", 1)):
            d["title"] = d["title"] + " (renamed)"
        if (d["id"], d["major"]) == ("sim.read_prob", 1):
            d["exposure"] = [x for x in d["exposure"] if x != "mcp"]
        b.append(d)
    person = [
        {"id": "align.set_datum", "major": 1, "kind": "changed", "compat": "additive", "fields": ["/title"]},
        {"id": "cmm.move_home", "major": 1, "kind": "added", "compat": "additive", "fields": []},
        {"id": "cmm.move_to", "major": 1, "kind": "removed", "compat": "breaking", "fields": []},
        {"id": "sim.read_prob", "major": 1, "kind": "changed", "compat": "breaking", "fields": ["/exposure"]},
        {"id": "sim.read_probe", "major": 1, "kind": "changed", "compat": "additive", "fields": ["/title"]},
    ]
    # Independent check of the audience rule: visible on every side that holds the command.
    def holds(ds, c):
        return [d for d in ds if (d["id"], d["major"]) == (c["id"], c["major"])]
    agent = [c for c in person if all(projection(d) != "never" for d in holds(a, c) + holds(b, c))]
    assert [c["id"] for c in agent] == ["sim.read_probe"], agent
    ta, tb = manifest_text(a), manifest_text(b)
    digests = {"a_manifest_digest": sha(ta), "b_manifest_digest": sha(tb)}
    return ta, tb, {"person": dict(digests, changes=person), "agent": dict(digests, changes=agent)}


def projection_table():
    """Every combination of class x declared projection x human_only x mcp in exposure, with the
    MCP projection the rule gives (`projection` above, written from CORE.BUS.POL.PROJ)."""
    rows = []
    for cls in sorted(CEILING):
        for declared in ["never", "request_only", "exposed"]:
            for human_only in [False, True]:
                for on_mcp in [False, True]:
                    d = {"side_effect": cls, "mcp_projection": declared, "human_only": human_only,
                         "exposure": ["cli", "mcp"] if on_mcp else ["cli"]}
                    rows.append({"side_effect": cls, "declared": declared, "human_only": human_only,
                                 "on_mcp": on_mcp, "expect": projection(d)})
    assert len(rows) == 11 * 3 * 2 * 2
    # Hand spot checks of the rule.
    pick = {(r["side_effect"], r["declared"], r["human_only"], r["on_mcp"]): r["expect"] for r in rows}
    assert pick[("W2", "exposed", True, True)] == "request_only"
    assert pick[("R0", "exposed", False, False)] == "never"
    assert pick[("W3", "exposed", False, True)] == "request_only"
    assert pick[("A0", "exposed", False, True)] == "never"
    assert pick[("R1", "request_only", False, True)] == "request_only"
    return {"rows": rows}


def render(doc):
    return json.dumps(doc, indent=1, ensure_ascii=False) + "\n"


def outputs():
    valid = json.loads(VALID.read_text(encoding="utf-8"))["decls"]
    qry = qry_decls(valid)
    ta, tb, aud = audience_diff(qry)
    return {
        "audience_a.manifest.json": ta,
        "audience_b.manifest.json": tb,
        "audience.changes.json": render(aud),
        "decls.json": render({"decls": qry}),
        "projection.json": render(projection_table()),
        "cases.json": render({"cases": cases({"valid": valid, "qry": qry})}),
        "v1_v2.changes.json": render(expected_changes()),
    }


def main():
    check = "--check" in sys.argv[1:]
    stale = 0
    for name, text in outputs().items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale += 1
                print(f"stale: {path}")
        else:
            path.write_bytes(data)
            print(f"wrote {path}")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
