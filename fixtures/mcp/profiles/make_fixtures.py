#!/usr/bin/env python3
"""Independent fixture generator for AUTO.MCP.SRV (AC-1..AC-7). CC-BY-4.0 project data.

Uses only Python's standard library (`hashlib`, `json`, `re`, `tomllib`) — zero shared code
with `crates/fid-mcp` — to generate:
  manifest.json            fixtures/manifest/min-classes.json + 4 tools.* commands
  core.toml                core MCP profile (AC-1)
  inspect.toml             inspect MCP profile (AC-1)
  expected.json            independently resolved RFC 8785 mcp-profiles.json (AC-1)
  too-many.toml            profile with 16 front-door entries (AC-2)
  manifest-many.json       manifest with 16 projectable commands for AC-2
  missing-disclosure.toml  profile omitting tools.search_tools@1 (AC-3)
  m1-front-door.toml       profile naming M1 demo.move_axis@1 in front_door (AC-4)
  manifest-m1-demo.json    manifest adding M1 demo.move_axis@1 for AC-4
  manifest-orphan.json     manifest adding projectable demo.orphan@1 for AC-5

Run:
  python fixtures/mcp/profiles/make_fixtures.py
  python fixtures/mcp/profiles/make_fixtures.py --check
"""

import argparse
import hashlib
import json
import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MIN_CLASSES = ROOT / "fixtures" / "manifest" / "min-classes.json"
SCHEMA_PATH = ROOT / "schemas" / "mcp-profile-1.json"
CORE_CRATE_PROFILE = ROOT / "crates" / "fid-mcp" / "profiles" / "core.toml"

CLASS_CEILING = {
    "R0": "exposed",
    "R1": "exposed",
    "I1": "exposed",
    "W1": "exposed",
    "W2": "exposed",
    "W3": "request_only",
    "X1": "request_only",
    "D1": "request_only",
    "M1": "never",
    "S1": "never",
    "A0": "never",
}

NAME_RE = re.compile(r"^[a-z][a-z0-9_]{1,31}$")
DOMAIN_RE = re.compile(r"^[a-z][a-z0-9_]{0,31}$")
CMD_RE = re.compile(r"^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$")


def canon(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def make_def(name: str, major: int, side_effect: str, declared: str, description: str) -> dict:
    d = {
        "algorithms": [],
        "description": description,
        "determinism": "D0",
        "effects": [],
        "exposure": ["cli", "mcp", "python"],
        "fs_effect": "none",
        "human_only": False,
        "id": name,
        "idempotency": "natural",
        "lifecycle": {"state": "active"},
        "major": major,
        "mcp_projection": declared,
        "params_schema": {"properties": {}, "type": "object"},
        "preconditions": [],
        "result_schema": {"type": "object"},
        "side_effect": side_effect,
        "title": f"Fixture {name}@{major}",
        "undo": "not_undoable",
        "web": "available",
    }
    d["definition_digest"] = sha(canon(d))
    return d


def extend_manifest(base: dict, extra_defs: list[dict]) -> bytes:
    cmds = list(base["commands"]) + list(extra_defs)
    cmds.sort(key=lambda d: (d["id"].encode("utf-8"), d["major"]))
    return canon({
        "commands": cmds,
        "format": base["format"],
        "quantity_catalogue_digest": base["quantity_catalogue_digest"],
    })


TOOLS_DEFS = [
    make_def("tools.describe_tool", 1, "R0", "exposed", "Describe one MCP tool definition."),
    make_def("tools.get_workflow", 1, "R0", "exposed", "Fetch a named metrology workflow guide."),
    make_def("tools.list_domains", 1, "R0", "exposed", "List command domains served by this profile."),
    make_def("tools.search_tools", 1, "R0", "exposed", "Search served MCP tools by keyword or domain."),
]

CORE_TOML = """\
# Fixture core profile for AUTO.MCP.SRV (AC-1).
format = "fid.mcp-profile"
version = 1
name = "core"
title = "Metrology Maven Core"
domains = [
  "tools",
  "project",
  "program",
  "egress",
  "rpt",
  "motion",
  "plugin",
  "trust",
]
commands = []
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "project.list_features@1",
  "program.save_draft@1",
]
"""

INSPECT_TOML = """\
# Fixture inspect profile for AUTO.MCP.SRV (AC-1).
format = "fid.mcp-profile"
version = 1
name = "inspect"
title = "Metrology Maven Inspect"
domains = [
  "tools",
  "align",
  "probe",
  "sim",
]
commands = [
  "project.list_features@1",
]
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "align.run_bestfit@1",
  "align.set_datum@1",
  "sim.run_program@1",
]
"""

TOO_MANY_TOML = """\
# Fixture profile exceeding the 15-tool front-door cap (AC-2).
format = "fid.mcp-profile"
version = 1
name = "too_many"
title = "Too Many Front-Door Tools"
domains = ["tools", "demo"]
commands = []
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "demo.cmd_01@1",
  "demo.cmd_02@1",
  "demo.cmd_03@1",
  "demo.cmd_04@1",
  "demo.cmd_05@1",
  "demo.cmd_06@1",
  "demo.cmd_07@1",
  "demo.cmd_08@1",
  "demo.cmd_09@1",
  "demo.cmd_10@1",
  "demo.cmd_11@1",
  "demo.cmd_12@1",
]
"""

MISSING_DISCLOSURE_TOML = """\
# Fixture profile omitting tools.search_tools@1 from front_door (AC-3).
format = "fid.mcp-profile"
version = 1
name = "missing_disc"
title = "Missing Disclosure Command"
domains = ["tools", "project"]
commands = []
front_door = [
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "project.list_features@1",
]
"""

M1_FRONT_DOOR_TOML = """\
# Fixture profile naming an M1 motion command in front_door (AC-4).
format = "fid.mcp-profile"
version = 1
name = "bad_motion"
title = "Illegal Motion Front Door"
domains = ["tools", "demo"]
commands = []
front_door = [
  "tools.search_tools@1",
  "tools.describe_tool@1",
  "tools.list_domains@1",
  "tools.get_workflow@1",
  "demo.move_axis@1",
]
"""


def validate_profile_schema(raw: dict, check_max_front_door: bool = True) -> None:
    allowed = {"format", "version", "name", "title", "domains", "commands", "front_door"}
    assert set(raw.keys()) <= allowed, f"extra keys: {set(raw.keys()) - allowed}"
    assert raw["format"] == "fid.mcp-profile"
    assert raw["version"] == 1
    assert NAME_RE.match(raw["name"])
    assert isinstance(raw["title"], str) and 1 <= len(raw["title"]) <= 120
    assert all(DOMAIN_RE.match(d) for d in raw["domains"])
    assert len(set(raw["domains"])) == len(raw["domains"])
    cmds = raw.get("commands", [])
    assert all(CMD_RE.match(c) for c in cmds)
    assert len(set(cmds)) == len(cmds)
    fd = raw["front_door"]
    assert all(CMD_RE.match(c) for c in fd)
    assert len(set(fd)) == len(fd)
    if check_max_front_door:
        assert len(fd) <= 15


def is_projectable(cmd: dict) -> bool:
    if "mcp" not in cmd["exposure"]:
        return False
    if cmd["mcp_projection"] == "never":
        return False
    return CLASS_CEILING[cmd["side_effect"]] != "never"


def reference_resolve(manifest_bytes: bytes, toml_texts: list[str]) -> bytes:
    doc = json.loads(manifest_bytes)
    projectable = {}
    by_domain: dict[str, list[str]] = {}
    for c in doc["commands"]:
        if is_projectable(c):
            cid = f"{c['id']}@{c['major']}"
            projectable[cid] = c
            dom = c["id"].split(".", 1)[0]
            by_domain.setdefault(dom, []).append(cid)

    tools_cmds = set(by_domain.get("tools", []))
    profiles_out = []
    specs = [tomllib.loads(t) for t in toml_texts]
    specs.sort(key=lambda s: s["name"].encode("utf-8"))

    for spec in specs:
        validate_profile_schema(spec, check_max_front_door=True)
        served = set()
        for dom in spec["domains"]:
            served.update(by_domain.get(dom, []))
        for extra in spec.get("commands", []):
            assert extra in projectable, f"unprojectable extra {extra}"
            served.add(extra)
        fd = spec["front_door"]
        assert len(fd) <= 15
        assert all(c in served and c in projectable for c in fd)
        assert tools_cmds <= set(fd)
        profiles_out.append({
            "front_door": fd,
            "name": spec["name"],
            "served": sorted(served),
            "title": spec["title"],
        })

    # Verify CI completeness: every projectable command is served by at least one profile.
    union = set().union(*(p["served"] for p in profiles_out))
    assert set(projectable.keys()) <= union

    return canon({
        "format": 1,
        "manifest_digest": sha(manifest_bytes),
        "profiles": profiles_out,
        "surface": "mcp",
    })


def expected_outputs() -> dict[str, bytes]:
    # Verify published schema JSON parses cleanly and crate's core.toml validates.
    schema = json.loads(SCHEMA_PATH.read_text(encoding="utf-8"))
    assert schema["$id"] == "https://metrologymaven.io/schemas/mcp-profile-1.json"
    validate_profile_schema(tomllib.loads(CORE_CRATE_PROFILE.read_text(encoding="utf-8")))

    base = json.loads(MIN_CLASSES.read_text(encoding="utf-8"))
    manifest_bytes = extend_manifest(base, TOOLS_DEFS)
    many_defs = TOOLS_DEFS + [
        make_def(f"demo.cmd_{i:02d}", 1, "R0", "exposed", f"Demo command {i:02d}.")
        for i in range(1, 13)
    ]
    manifest_many_bytes = extend_manifest(base, many_defs)
    manifest_m1_bytes = extend_manifest(
        base,
        TOOLS_DEFS + [make_def("demo.move_axis", 1, "M1", "never", "Motion axis move (M1).")],
    )
    manifest_orphan_bytes = extend_manifest(
        base,
        TOOLS_DEFS + [make_def("demo.orphan", 1, "R0", "exposed", "Orphan projectable command.")],
    )

    validate_profile_schema(tomllib.loads(TOO_MANY_TOML), check_max_front_door=False)
    validate_profile_schema(tomllib.loads(MISSING_DISCLOSURE_TOML), check_max_front_door=True)
    validate_profile_schema(tomllib.loads(M1_FRONT_DOOR_TOML), check_max_front_door=True)

    expected_json = reference_resolve(manifest_bytes, [CORE_TOML, INSPECT_TOML])

    return {
        "manifest.json": manifest_bytes,
        "manifest-many.json": manifest_many_bytes,
        "manifest-m1-demo.json": manifest_m1_bytes,
        "manifest-orphan.json": manifest_orphan_bytes,
        "core.toml": CORE_TOML.encode("utf-8"),
        "inspect.toml": INSPECT_TOML.encode("utf-8"),
        "too-many.toml": TOO_MANY_TOML.encode("utf-8"),
        "missing-disclosure.toml": MISSING_DISCLOSURE_TOML.encode("utf-8"),
        "m1-front-door.toml": M1_FRONT_DOOR_TOML.encode("utf-8"),
        "expected.json": expected_json,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify fixtures match regenerated bytes")
    args = ap.parse_args()

    outputs = expected_outputs()
    if args.check:
        ok = True
        for name, data in sorted(outputs.items()):
            p = HERE / name
            if not p.exists():
                print(f"MISSING {p}", file=sys.stderr)
                ok = False
            elif p.read_bytes() != data:
                print(f"DRIFT {p}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    HERE.mkdir(parents=True, exist_ok=True)
    for name, data in sorted(outputs.items()):
        (HERE / name).write_bytes(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
