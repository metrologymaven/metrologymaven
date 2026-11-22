#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for AUTO.SURF.CHECK (AC-1..AC-5).

Builds four fixture trees under `fixtures/surfaces/` from `fixtures/manifest/min-classes.json`
and `fixtures/mcp/min-classes.tools.json`:
- `complete/`: complete `cli`, `mcp`, `py`, `docs` surface indexes + exact `mcp/mcp-tools.json`
- `missing-py/`: identical to `complete/`, with `align.run_bestfit@1` removed from `py/surface-index.json`
- `orphan/`: identical to `complete/`, with `demo.ghost_cmd@1` added to `cli/surface-index.json`
- `m1-listed/`: identical to `complete/`, with M1 command `motion.move_axis@1` added to `mcp/surface-index.json`

Usage: python3 fixtures/surfaces/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
HERE = Path(__file__).resolve().parent
MANIFEST = ROOT / "fixtures" / "manifest" / "min-classes.json"
MCP_TOOLS = ROOT / "fixtures" / "mcp" / "min-classes.tools.json"
DOCS_FIXTURES = ROOT / "fixtures" / "docs"

NON_MCP_CLASSES = {"M1", "S1", "A0"}


def canonical_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def index_bytes(manifest_digest: str, surface: str, commands: list[tuple[str, int]]) -> bytes:
    ordered = sorted(set(commands), key=lambda x: x[0])
    doc = {
        "_header": f"generated, do not edit; manifest {manifest_digest}; generator fid-surface-gen 1",
        "commands": [{"command_id": cid, "major": major} for cid, major in ordered],
        "generator_version": "1",
        "manifest_digest": manifest_digest,
        "surface": surface,
    }
    return canonical_bytes(doc)


def docs_generated_files() -> dict[Path, bytes]:
    out: dict[Path, bytes] = {}
    classes = DOCS_FIXTURES / "classes.json"
    out[Path("classes.json")] = classes.read_bytes()
    for sub in ("ref", "index"):
        d = DOCS_FIXTURES / sub
        for p in sorted(d.glob("*.json")):
            out[Path(sub) / p.name] = p.read_bytes()
    return out


def build_trees() -> dict[Path, bytes]:
    manifest_raw = MANIFEST.read_bytes()
    manifest_digest = "sha256:" + hashlib.sha256(manifest_raw).hexdigest()
    manifest = json.loads(manifest_raw.decode("utf-8"))
    mcp_tools_raw = MCP_TOOLS.read_bytes()
    docs_files = docs_generated_files()

    all_cmds: list[tuple[str, int]] = []
    mcp_cmds: list[tuple[str, int]] = []
    for c in manifest["commands"]:
        cid = f"{c['id']}@{c['major']}"
        major = int(c["major"])
        all_cmds.append((cid, major))
        if (
            c["side_effect"] not in NON_MCP_CLASSES
            and c["mcp_projection"] != "never"
            and "mcp" in c.get("exposure", [])
        ):
            mcp_cmds.append((cid, major))

    def make_tree(
        name: str,
        cli_cmds: list[tuple[str, int]],
        py_cmds: list[tuple[str, int]],
        docs_cmds: list[tuple[str, int]],
        tree_mcp_cmds: list[tuple[str, int]],
    ) -> dict[Path, bytes]:
        base = HERE / name
        tree = {
            base / "cli" / "surface-index.json": index_bytes(manifest_digest, "cli", cli_cmds),
            base / "py" / "surface-index.json": index_bytes(manifest_digest, "py", py_cmds),
            base / "docs" / "surface-index.json": index_bytes(manifest_digest, "docs", docs_cmds),
            base / "mcp" / "surface-index.json": index_bytes(manifest_digest, "mcp", tree_mcp_cmds),
            base / "mcp" / "mcp-tools.json": mcp_tools_raw,
        }
        for rel, data in docs_files.items():
            tree[base / "docs" / rel] = data
        return tree

    files: dict[Path, bytes] = {}
    files.update(make_tree("complete", all_cmds, all_cmds, all_cmds, mcp_cmds))
    files.update(
        make_tree(
            "missing-py",
            all_cmds,
            [c for c in all_cmds if c[0] != "align.run_bestfit@1"],
            all_cmds,
            mcp_cmds,
        )
    )
    files.update(
        make_tree(
            "orphan",
            all_cmds + [("demo.ghost_cmd@1", 1)],
            all_cmds,
            all_cmds,
            mcp_cmds,
        )
    )
    files.update(
        make_tree(
            "m1-listed",
            all_cmds,
            all_cmds,
            all_cmds,
            mcp_cmds + [("motion.move_axis@1", 1)],
        )
    )
    return files


def main() -> int:
    files = build_trees()
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(files.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale surface fixtures: {', '.join(stale)}")
            return 1
        print("fixtures/surfaces: ok")
        return 0
    for path, want in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(files)} fixture files under {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
