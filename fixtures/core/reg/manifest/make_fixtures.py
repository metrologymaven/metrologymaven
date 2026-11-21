#!/usr/bin/env python3
"""Generate the CORE.REG.MAN fixtures (CC-BY-4.0 project data). Stdlib only.

This is an independent reference: the expected manifest bytes and digests are computed here
from the rules of spec/core/reg/man/NODE.md with Python's own JSON and hashlib, never by the
Rust exporter.

- valid.manifest.json: the manifest of ../decls/valid.json (exact bytes, no newline).
- valid.digest: its digest, `sha256:<hex>` and one LF (AC-1).
- order.json / order.manifest.json / order.digest: declarations that exercise the normal
  form (exposure listed out of surface order, optional fields omitted, a precondition and an
  algorithm), in shuffled order.
- tampered.json: valid.manifest.json with one title changed and its definition_digest kept
  (AC-4: `manifest_invalid`).
- visibility.json / visibility.manifest.json / visibility.digest / visibility.agent.json /
  visibility.agent.manifest.json / visibility.agent.digest: registry visibility (D-035,
  CONSTITUTION section 8). visibility.json holds declarations of every side-effect class,
  with and without `mcp` among their surfaces, with `human_only`, and deprecated majors whose
  `replaced_by` names a visible command, a hidden command and an absent command.
  visibility.manifest.json is the full manifest (what a person gets); visibility.agent.json
  lists the (id, major) pairs an agent sees, and visibility.agent.manifest.json is the agent
  view: the manifest of those declarations only, each `replaced_by` that names a command
  outside the view removed. Its digest is the digest of the agent view's bytes.

Rules (NODE.md Design, pinned choices):
- each declaration in normal form: `human_only` false, `network` "none", `cli_bare` false
  when absent; `exposure` in surface order cli, mcp, python, gui, control; every other field
  as declared;
- definition_digest = SHA-256 of the RFC 8785 bytes of that normal form;
- commands sorted by id (byte order) then major;
- manifest = {"commands", "format": 1, "quantity_catalogue_digest"} in RFC 8785 form, where
  the catalogue digest is the `GEO.FRAME.UNITS` catalogue digest of
  ../def/schema/catalogue/geo_frame_units.sha256 (itself computed with hashlib).
- MCP projection (CORE.BUS.POL.PROJ, written out again here from the spec): `never` when
  `mcp` is not among the surfaces; otherwise the stricter of the declared projection
  (`human_only` lowers `exposed` to `request_only`) and the class ceiling (R0 R1 I1 W1 W2
  exposed; W3 X1 D1 request_only; M1 S1 A0 never). An agent sees a command exactly when that
  projection is not `never`.

Run:  python fixtures/core/reg/manifest/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
DECLS = HERE.parent / "decls" / "valid.json"
CATALOGUE_DIGEST = HERE.parent / "def" / "schema" / "catalogue" / "geo_frame_units.sha256"
SURFACES = ["cli", "mcp", "python", "gui", "control"]


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
        if v != v or v in (float("inf"), float("-inf")) or v != int(v) or abs(v) >= 1e21:
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


def manifest(decls, catalogue_digest):
    commands = []
    for decl in sorted(decls, key=lambda d: (d["id"].encode("utf-8"), d["major"])):
        body = normal(decl)
        body["definition_digest"] = sha(jcs(body))
        commands.append(body)
    return jcs({"format": 1, "quantity_catalogue_digest": catalogue_digest, "commands": commands})


PROJECTIONS = ["never", "request_only", "exposed"]
CEILING = {
    "R0": "exposed", "R1": "exposed", "I1": "exposed", "W1": "exposed", "W2": "exposed",
    "W3": "request_only", "X1": "request_only", "D1": "request_only",
    "M1": "never", "S1": "never", "A0": "never",
}


def mcp_projection(decl):
    if "mcp" not in decl["exposure"]:
        return "never"
    declared = decl["mcp_projection"]
    if decl.get("human_only", False) and declared == "exposed":
        declared = "request_only"
    ceiling = CEILING[decl["side_effect"]]
    return min(declared, ceiling, key=PROJECTIONS.index)


def agent_view(decls):
    seen = [copy.deepcopy(d) for d in decls if mcp_projection(d) != "never"]
    visible = {f"{d['id']}@{d['major']}" for d in seen}
    for d in seen:
        target = d["lifecycle"].get("replaced_by")
        if target is not None and target not in visible:
            del d["lifecycle"]["replaced_by"]
    return seen


def visibility_decls(decls):
    """valid.json's twelve declarations (one per class), all on cli and mcp, plus variants."""
    out = []
    for d in decls:
        d = copy.deepcopy(d)
        d["exposure"] = ["mcp", "cli"]
        out.append(d)
    by = {(d["id"], d["major"]): d for d in out}
    # human_only: an exposed W2 becomes request_only (still visible); an S1 stays never.
    by[("align.run_bestfit", 1)]["human_only"] = True
    by[("trust.sign_record", 1)]["human_only"] = True
    # Deprecated majors whose replaced_by names a visible, a hidden and an absent command.
    life = {"state": "deprecated", "since": "2026-06-01"}
    by[("project.list_features", 1)]["lifecycle"] = dict(life, replaced_by="project.list_features@2")
    by[("sim.read_probe", 1)]["lifecycle"] = dict(life, replaced_by="cmm.move_to@1")
    by[("report.set_title", 1)]["lifecycle"] = dict(life, replaced_by="report.set_heading@1", remove_after="2027-06-01")
    # Exposed R0 commands that are not on mcp, or declared never: hidden.
    cli_only = copy.deepcopy(by[("gdt.evaluate_flatness", 1)])
    cli_only.update(id="gdt.evaluate_cli_only", title="Fixture gdt.evaluate_cli_only@1", exposure=["cli", "gui"])
    never = copy.deepcopy(by[("gdt.evaluate_flatness", 1)])
    never.update(id="gdt.evaluate_never", title="Fixture gdt.evaluate_never@1", mcp_projection="never")
    # A deprecated visible major replaced by a declared-never command.
    to_never = copy.deepcopy(by[("gdt.evaluate_flatness", 1)])
    to_never.update(
        id="gdt.evaluate_old",
        title="Fixture gdt.evaluate_old@1",
        lifecycle=dict(life, replaced_by="gdt.evaluate_never@1"),
    )
    out += [cli_only, never, to_never]
    return list(reversed(out))


def order_decls(base):
    """Three declarations built from valid.json's R0 declaration, in shuffled order."""
    a = copy.deepcopy(base)
    a.update(id="zeta.show_item", title="Zeta show item", exposure=["python", "mcp", "cli"])
    for k in ("human_only", "network", "cli_bare"):
        a.pop(k, None)
    b = copy.deepcopy(base)
    b.update(
        id="alpha.check_item",
        major=3,
        title="Alpha check item",
        exposure=["gui", "cli"],
        preconditions=[{"expr": "true", "bound": 10, "message_key": "alpha.not_ready"}],
        algorithms=["fit.plane.ls@1.0.0"],
        human_only=True,
        network="none",
        cli_bare=False,
    )
    c = copy.deepcopy(base)
    c.update(id="alpha.check_item", major=1, title="Alpha check item v1", exposure=["control", "mcp"])
    return [a, b, c]


def outputs():
    catalogue_digest = CATALOGUE_DIGEST.read_text(encoding="utf-8").strip()
    decls = json.loads(DECLS.read_text(encoding="utf-8"))["decls"]
    valid = manifest(decls, catalogue_digest)
    order = order_decls(decls[0])
    order_manifest = manifest(order, catalogue_digest)
    vis = visibility_decls(decls)
    vis_manifest = manifest(vis, catalogue_digest)
    agent = agent_view(vis)
    agent_manifest = manifest(agent, catalogue_digest)
    agent_ids = sorted(([d["id"], d["major"]] for d in agent), key=lambda p: (p[0].encode("utf-8"), p[1]))
    doc = json.loads(valid)
    doc["commands"][0]["title"] = doc["commands"][0]["title"] + " (tampered)"
    return {
        "valid.manifest.json": valid,
        "valid.digest": sha(valid) + "\n",
        "order.json": json.dumps({"decls": order}, indent=1, ensure_ascii=False) + "\n",
        "order.manifest.json": order_manifest,
        "order.digest": sha(order_manifest) + "\n",
        "tampered.json": jcs(doc),
        "visibility.json": json.dumps({"decls": vis}, indent=1, ensure_ascii=False) + "\n",
        "visibility.manifest.json": vis_manifest,
        "visibility.digest": sha(vis_manifest) + "\n",
        "visibility.agent.json": json.dumps({"visible": agent_ids}, indent=1) + "\n",
        "visibility.agent.manifest.json": agent_manifest,
        "visibility.agent.digest": sha(agent_manifest) + "\n",
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
