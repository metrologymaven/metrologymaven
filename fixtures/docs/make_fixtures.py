#!/usr/bin/env python3
"""Independent stdlib-Python reference generator for AUTO.SURF.DOCS (`docs-ref-v1`).

Reads `fixtures/manifest/min-classes.json` (never runs Rust) and computes from the spec rules:
- `fixtures/manifest/human-only.json`: synthetic manifest containing a W2 `human_only: true`
  command for AC-3 (plus R0 and M1 reference commands).
- `fixtures/docs/ref/<command_id>@<major>.json`: golden `docs-ref-v1` records for every command
  major in `fixtures/manifest/min-classes.json`, stamped with the `fid-surface-gen` `_header`.
- `fixtures/docs/index/<domain>.json`: golden per-domain indexes in byte order.
- `fixtures/docs/classes.json`: golden side-effect class legend (CONSTITUTION §3).
- `fixtures/docs/surface-index.json`: golden docs surface index.

Usage:
  python3 fixtures/docs/make_fixtures.py          # write fixtures
  python3 fixtures/docs/make_fixtures.py --check  # fail if any committed fixture differs

Licence: CC-BY-4.0 (synthetic, project-authored).
"""

import hashlib
import json
import keyword
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS_DIR = ROOT / "fixtures" / "docs"
MAN_DIR = ROOT / "fixtures" / "manifest"
MIN_CLASSES = MAN_DIR / "min-classes.json"
HUMAN_ONLY_MANIFEST = MAN_DIR / "human-only.json"

QCD = "sha256:" + "ab" * 32

CLASSES = [
    ("R0", "exposed"),
    ("R1", "exposed"),
    ("W1", "named"),
    ("W2", "named"),
    ("W3", "request_only"),
    ("X1", "request_only"),
    ("I1", "exposed"),
    ("D1", "request_only"),
    ("M1", "none"),
    ("S1", "none"),
    ("A0", "none"),
]
CLASS_PROJ = dict(CLASSES)
DECLARED_ORDER = {"never": 0, "request_only": 1, "exposed": 2}
CEILING_DECLARED = {"exposed": "exposed", "named": "exposed", "request_only": "request_only", "none": "never"}


def _jcs_norm(v):
    if isinstance(v, bool) or v is None or isinstance(v, (int, str)):
        return v
    if isinstance(v, float):
        if v.is_integer():
            return int(v)
        return v
    if isinstance(v, list):
        return [_jcs_norm(x) for x in v]
    if isinstance(v, dict):
        return {k: _jcs_norm(val) for k, val in v.items()}
    raise TypeError(f"unsupported JSON value: {type(v)}")


def canon(v) -> bytes:
    return json.dumps(_jcs_norm(v), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def py_ident(seg: str) -> str:
    return seg + "_" if keyword.iskeyword(seg) else seg


def type_of(schema: dict) -> str:
    t = schema.get("type")
    if isinstance(t, str):
        return t
    if isinstance(t, list):
        return "|".join(sorted(set(t)))
    if "$ref" in schema:
        return schema["$ref"]
    return "any"


def effective_mcp_projection(side_effect: str, human_only: bool, declared: str) -> str:
    ceiling = CLASS_PROJ[side_effect]
    if human_only and ceiling in ("exposed", "named"):
        ceiling = "request_only"
    eff_decl = "request_only" if (human_only and declared == "exposed") else declared
    c_decl = CEILING_DECLARED[ceiling]
    if DECLARED_ORDER[eff_decl] == DECLARED_ORDER[c_decl]:
        return ceiling
    if eff_decl == "never":
        return "none"
    return "request_only"


def stamp_json_member(header_text: str, body: bytes) -> bytes:
    assert body.startswith(b"{")
    header_prefix = (
        '{"_header":' + json.dumps(header_text, ensure_ascii=False) + ","
    ).encode("utf-8")
    if body == b"{}":
        return header_prefix[:-1] + b"}\n"
    return header_prefix + body[1:] + b"\n"


def make_human_only_manifest() -> bytes:
    def definition(cid, major, se, declared, human_only, title, description, params=None, required=()):
        d = {
            "algorithms": [],
            "description": description,
            "determinism": "D0",
            "effects": [],
            "exposure": ["cli", "mcp", "python"],
            "fs_effect": "none",
            "human_only": human_only,
            "id": cid,
            "idempotency": "natural",
            "lifecycle": {"state": "active"},
            "major": major,
            "mcp_projection": declared,
            "params_schema": {"properties": params or {}, "type": "object"},
            "preconditions": [],
            "result_schema": {"type": "object"},
            "side_effect": se,
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
            "align.adopt_datum_frame",
            1,
            "W2",
            "exposed",
            True,
            "Adopt datum frame onto main document",
            "Promotes a reviewed datum frame from an agent branch into the record document.",
            params={"frame_id": {"type": "string"}},
            required=("frame_id",),
        ),
        definition(
            "motion.move_axis",
            1,
            "M1",
            "never",
            False,
            "Move machine axis",
            "Direct machine-motion command, never reachable on MCP.",
        ),
        definition(
            "project.list_features",
            1,
            "R0",
            "exposed",
            False,
            "List project features",
            "Returns every feature handle in the active document.",
        ),
    ]
    cmds.sort(key=lambda c: (c["id"].encode("utf-8"), c["major"]))
    return canon({"commands": cmds, "format": 1, "quantity_catalogue_digest": QCD})


def build_docs_tree(manifest_bytes: bytes) -> dict[str, bytes]:
    manifest_digest = sha256(manifest_bytes)
    header_text = f"generated, do not edit; manifest {manifest_digest}; generator fid-surface-gen 1"
    doc = json.loads(manifest_bytes.decode("utf-8"))
    cmds = sorted(doc["commands"], key=lambda c: (c["id"].encode("utf-8"), int(c["major"])))

    files: dict[str, bytes] = {}
    by_domain: dict[str, list[dict]] = {}
    covered: list[tuple[str, int]] = []

    for c in cmds:
        cid = c["id"]
        major = int(c["major"])
        full_id = f"{cid}@{major}"
        covered.append((full_id, major))
        domain, verb = cid.split(".", 1)
        suffix = f"_v{major}" if major >= 2 else ""

        cli_name = f"fid {domain} {verb.replace('_', '-')}"
        mcp_name = f"{domain}_{verb}{suffix}"
        py_name = f"fiducial.{py_ident(domain)}.{py_ident(verb + suffix)}"

        se = c["side_effect"]
        det = c["determinism"]
        human_only = bool(c.get("human_only", False))
        exposure = list(c.get("exposure", []))
        mcp_proj = effective_mcp_projection(se, human_only, c["mcp_projection"])

        cli_doc = {"available": True, "name": cli_name} if "cli" in exposure else {"available": False}
        py_doc = {"available": True, "name": py_name} if "python" in exposure else {"available": False}

        if mcp_proj == "none" or "mcp" not in exposure:
            mcp_doc = {"available": False, "projection": "none", "reason": "class_never_on_mcp"}
        elif human_only:
            mcp_doc = {"available": True, "name": mcp_name, "projection": mcp_proj, "reason": "human_only"}
        elif mcp_proj == "request_only":
            mcp_doc = {"available": True, "name": mcp_name, "projection": "request_only", "reason": "request_only"}
        else:
            mcp_doc = {"available": True, "name": mcp_name, "projection": mcp_proj}

        props = c.get("params_schema", {}).get("properties", {})
        params = []
        for pname in sorted(props.keys()):
            ps = props[pname]
            params.append(
                {
                    "default": ps["default"] if "default" in ps else None,
                    "name": pname,
                    "quantity_kind": ps.get("x-quantity"),
                    "ty": type_of(ps),
                    "unit": ps.get("x-unit"),
                }
            )

        rec = {
            "command_digest": c["definition_digest"],
            "command_id": cid,
            "determinism": det,
            "dry_run": False,
            "help": c["description"],
            "human_only": human_only,
            "long_running": False,
            "major": major,
            "manifest_digest": manifest_digest,
            "params": params,
            "result": type_of(c["result_schema"]),
            "schema": "docs-ref-v1",
            "side_effect": se,
            "summary": c["title"],
            "surfaces": {"cli": cli_doc, "mcp": mcp_doc, "python": py_doc},
        }
        ref_rel = f"ref/{cid}@{major}.json"
        files[ref_rel] = stamp_json_member(header_text, canon(rec))

        by_domain.setdefault(domain, []).append(
            {
                "command_id": cid,
                "major": major,
                "ref_path": ref_rel,
                "summary": c["title"],
            }
        )

    for domain in sorted(by_domain.keys()):
        entries = sorted(by_domain[domain], key=lambda e: (e["command_id"].encode("utf-8"), e["major"]))
        idx_doc = {
            "commands": entries,
            "domain": domain,
            "manifest_digest": manifest_digest,
        }
        files[f"index/{domain}.json"] = stamp_json_member(header_text, canon(idx_doc))

    legend = {"classes": [{"code": code, "mcp_projection": proj} for code, proj in CLASSES]}
    files["classes.json"] = stamp_json_member(header_text, canon(legend))

    ordered_covered = sorted(set(covered), key=lambda x: x[0].encode("utf-8"))
    surface_index = {
        "_header": header_text,
        "commands": [{"command_id": cid, "major": major} for cid, major in ordered_covered],
        "generator_version": "1",
        "manifest_digest": manifest_digest,
        "surface": "docs",
    }
    files["surface-index.json"] = canon(surface_index)
    return files


def outputs() -> dict[Path, bytes]:
    min_bytes = MIN_CLASSES.read_bytes()
    out: dict[Path, bytes] = {
        HUMAN_ONLY_MANIFEST: make_human_only_manifest(),
        DOCS_DIR / "LICENSE.txt": (
            "Synthetic, project-authored test fixtures for spec node AUTO.SURF.DOCS.\n"
            "Licensed under CC-BY-4.0.\n"
        ).encode("utf-8"),
    }
    for rel, data in build_docs_tree(min_bytes).items():
        out[DOCS_DIR / rel] = data
    return out


def main() -> int:
    files = outputs()
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(files.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale AUTO.SURF.DOCS fixtures: {', '.join(stale)}")
            return 1
        print(f"fixtures/docs: ok ({len(files)} files)")
        return 0
    for path, want in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(files)} fixture files under {DOCS_DIR} and {HUMAN_ONLY_MANIFEST.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
