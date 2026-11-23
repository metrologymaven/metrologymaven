#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for AUTO.CLI.TREE.

Reads `fixtures/manifest/min-classes.json` (never runs Rust) and produces:
- `fixtures/manifest/two-majors.json`: synthetic manifest with `demo.show_thing@1` and `@2` (AC-3).
- `fixtures/cli/min-classes.tree.json`: golden generated `cli-tree.json` (with `fid-surface-gen` `_header`).
- `fixtures/cli/two-majors.tree.json`: golden `cli-tree.json` for `two-majors.json`.
- `fixtures/cli/help/align-run-bestfit.txt`: golden `--help` output for `fid align run-bestfit --help` (AC-2).
- `fixtures/cli/help/align-set-datum.txt`: golden `--help` output for W2 command `fid align set-datum --help`.
- `fixtures/cli/LICENSE.txt`: CC-BY-4.0 notice.

Usage:
  python3 fixtures/cli/make_fixtures.py          # write fixtures
  python3 fixtures/cli/make_fixtures.py --check  # fail if any committed fixture differs
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CLI_DIR = ROOT / "fixtures" / "cli"
MAN_DIR = ROOT / "fixtures" / "manifest"
MIN_CLASSES = MAN_DIR / "min-classes.json"
TWO_MAJORS = MAN_DIR / "two-majors.json"

QCD = "sha256:" + "ab" * 32


def _jcs_norm(v):
    if isinstance(v, bool) or v is None or isinstance(v, (int, str)):
        return v
    if isinstance(v, float):
        return int(v) if v.is_integer() else v
    if isinstance(v, list):
        return [_jcs_norm(x) for x in v]
    if isinstance(v, dict):
        return {k: _jcs_norm(val) for k, val in v.items()}
    raise TypeError(f"unsupported value: {type(v)}")


def canon(v) -> bytes:
    return json.dumps(_jcs_norm(v), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def type_of(schema: dict) -> str:
    t = schema.get("type")
    if isinstance(t, str):
        return t
    if isinstance(t, list):
        return "|".join(sorted(set(t)))
    if "$ref" in schema:
        return schema["$ref"]
    return "any"


def stamp_json_member(header_text: str, body: bytes) -> bytes:
    assert body.startswith(b"{")
    prefix = ('{"_header":' + json.dumps(header_text, ensure_ascii=False) + ",").encode("utf-8")
    if body == b"{}":
        return prefix[:-1] + b"}\n"
    return prefix + body[1:] + b"\n"


def make_two_majors_manifest() -> bytes:
    def definition(cid, major, title, description, params=None, required=()):
        d = {
            "algorithms": [],
            "description": description,
            "determinism": "D0",
            "effects": [],
            "exposure": ["cli", "mcp", "python"],
            "fs_effect": "none",
            "human_only": False,
            "id": cid,
            "idempotency": "natural",
            "lifecycle": {"state": "active"},
            "major": major,
            "mcp_projection": "exposed",
            "params_schema": {"properties": params or {}, "type": "object"},
            "preconditions": [],
            "result_schema": {"type": "object"},
            "side_effect": "R0",
            "title": title,
            "undo": "not_undoable",
            "web": "available",
        }
        if required:
            d["params_schema"]["required"] = list(required)
        d["definition_digest"] = sha256(canon(d))
        return d

    cmds = [
        definition(
            "demo.show_thing",
            1,
            "Show thing (v1)",
            "Legacy major 1 view of a demo thing.",
            params={"item": {"type": "string"}},
            required=("item",),
        ),
        definition(
            "demo.show_thing",
            2,
            "Show thing (v2)",
            "Current major 2 view of a demo thing.",
            params={"item": {"type": "string"}, "verbose": {"default": False, "type": "boolean"}},
            required=("item",),
        ),
    ]
    return canon({"commands": cmds, "format": 1, "quantity_catalogue_digest": QCD})


def build_tree_object(manifest_bytes: bytes) -> tuple[str, dict]:
    manifest_digest = sha256(manifest_bytes)
    doc = json.loads(manifest_bytes.decode("utf-8"))
    cmds = sorted(doc["commands"], key=lambda c: (c["id"].encode("utf-8"), int(c["major"])))

    grouped: dict[tuple[str, str], dict] = {}
    for c in cmds:
        cid = c["id"]
        major = int(c["major"])
        domain, verb_raw = cid.split(".", 1)
        verb = verb_raw.replace("_", "-")
        path = ["fid", domain, verb]
        props = c.get("params_schema", {}).get("properties", {})
        req_set = set(c.get("params_schema", {}).get("required", []))
        params = []
        for pname in sorted(props.keys()):
            ps = props[pname]
            params.append(
                {
                    "default": ps["default"] if "default" in ps else None,
                    "name": pname,
                    "quantity_kind": ps.get("x-quantity"),
                    "required": pname in req_set,
                    "ty": type_of(ps),
                    "unit": ps.get("x-unit"),
                }
            )
        entry = grouped.setdefault((domain, verb), {"command_id": cid, "majors": [], "path": path})
        entry["majors"].append(
            {
                "command_id": f"{cid}@{major}",
                "determinism": c["determinism"],
                "dry_run": False,
                "help": c["description"],
                "human_only": bool(c.get("human_only", False)),
                "major": major,
                "params": params,
                "side_effect": c["side_effect"],
                "summary": c["title"],
            }
        )

    domains: dict[str, list[dict]] = {}
    for (domain, _verb) in sorted(grouped.keys()):
        cmd_obj = grouped[(domain, _verb)]
        cmd_obj["majors"].sort(key=lambda m: m["major"])
        domains.setdefault(domain, []).append(cmd_obj)

    tree = {"domains": domains, "manifest_digest": manifest_digest, "schema": "cli-tree-v1"}
    return manifest_digest, tree


def build_tree_bytes(manifest_bytes: bytes) -> tuple[dict, bytes]:
    manifest_digest, tree = build_tree_object(manifest_bytes)
    header = f"generated, do not edit; manifest {manifest_digest}; generator fid-surface-gen 1"
    return tree, stamp_json_member(header, canon(tree))


def render_help(tree: dict, domain: str, verb: str, major: int | None = None) -> bytes:
    cmds = tree["domains"][domain]
    cmd = next(c for c in cmds if c["path"][2] == verb)
    majors = cmd["majors"]
    default_major = majors[-1]["major"]
    sel = majors[-1] if major is None else next(m for m in majors if m["major"] == major)
    lines = [
        " ".join(cmd["path"]),
        sel["summary"],
        "",
        sel["help"],
        "",
        f"Command: {sel['command_id']}",
        f"Manifest: {tree['manifest_digest']}",
        f"Side-effect class: {sel['side_effect']}",
        f"Determinism: {sel['determinism']}",
        f"Dry run: {'supported' if sel['dry_run'] else 'unsupported'}",
        f"Majors: {', '.join(str(m['major']) for m in majors)} (default: {default_major})",
        "",
        "Parameters:",
    ]
    if not sel["params"]:
        lines.append("    (none)")
    else:
        for p in sel["params"]:
            meta = [p["ty"]]
            if p["quantity_kind"] is not None:
                meta.append(f"quantity={p['quantity_kind']}")
            if p["unit"] is not None:
                meta.append(f"unit={p['unit']}")
            if p["required"]:
                meta.append("required")
            if p["default"] is not None:
                meta.append("default=" + canon(p["default"]).decode("utf-8"))
            lines.append(f"    --{p['name'].replace('_', '-')} ({', '.join(meta)})")
    return ("\n".join(lines) + "\n").encode("utf-8")


def outputs() -> dict[Path, bytes]:
    min_bytes = MIN_CLASSES.read_bytes()
    two_bytes = make_two_majors_manifest()
    min_tree, min_tree_bytes = build_tree_bytes(min_bytes)
    _two_tree, two_tree_bytes = build_tree_bytes(two_bytes)
    return {
        TWO_MAJORS: two_bytes,
        CLI_DIR / "min-classes.tree.json": min_tree_bytes,
        CLI_DIR / "two-majors.tree.json": two_tree_bytes,
        CLI_DIR / "help" / "align-run-bestfit.txt": render_help(min_tree, "align", "run-bestfit"),
        CLI_DIR / "help" / "align-set-datum.txt": render_help(min_tree, "align", "set-datum"),
        CLI_DIR / "LICENSE.txt": (
            "Synthetic, project-authored test fixtures for spec node AUTO.CLI.TREE.\n"
            "Licensed under CC-BY-4.0.\n"
        ).encode("utf-8"),
    }


def main() -> int:
    files = outputs()
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(files.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale AUTO.CLI.TREE fixtures: {', '.join(stale)}")
            return 1
        print(f"fixtures/cli: ok ({len(files)} files)")
        return 0
    for path, want in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(files)} fixture files under {CLI_DIR} and {TWO_MAJORS.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
