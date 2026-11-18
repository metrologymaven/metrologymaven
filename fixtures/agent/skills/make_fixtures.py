#!/usr/bin/env python3
"""Generate the fixtures of spec node AGENT.SKILLS.CATALOG under fixtures/agent/skills/.

Project-authored synthetic data (CC-BY-4.0). The golden version index is computed here,
independently of the Rust crate, from the rules of REQ-AGENT.SKILLS.CATALOG-004:

  manifest digest  = SHA-256 of the exact manifest bytes (REQ-CORE.REG.MAN-002)
  catalog version  = SHA-256 of RFC 8785 JSON of
                     {"guide": <task guide declaration from the manifest>,
                      "recipes": [{"id": <recipe id>, "sha256": SHA-256 of the recipe file bytes}],
                      "row": {"guide_id", "profile", "recipes", "title", "workflow_id"}}
  catalog-index    = RFC 8785 JSON of {"manifest_digest", "rows": [{"catalog_version", "workflow_id"}]}
                     sorted by workflow ID, no trailing newline

All values are strings or small integers, so json.dumps with sorted keys and no spaces is RFC 8785.
Run:  python fixtures/agent/skills/make_fixtures.py
"""

import hashlib
import json
import shutil
from pathlib import Path

HERE = Path(__file__).resolve().parent


def canon(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def write(name, data):
    p = HERE / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)


def pretty(obj):
    return (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


# ---------------------------------------------------------------- manifest

COMMANDS = [
    {"id": "core.canon", "major": 1, "side_effect": "R0"},
    {"id": "demo.align", "major": 1, "side_effect": "R1"},
    {"id": "demo.compare", "major": 1, "side_effect": "R1"},
    {"id": "demo.export_report", "major": 1, "side_effect": "W1"},
    {"id": "demo.fit_cylinder", "major": 1, "side_effect": "R1"},
    {"id": "demo.fit_plane", "major": 1, "side_effect": "R1"},
    {"id": "demo.scan", "major": 1, "side_effect": "R1"},
]


def guide(gid, profile, title, steps):
    return {"id": gid, "profile": profile, "title": title, "summary": f"Synthetic fixture guide: {title}.",
            "steps": [{"command": {"id": c, "major": 1}, "purpose": p} for c, p in steps]}


GUIDES = [
    guide("core.canon_text", "core", "Canonicalise a JSON text", [("core.canon", "Print the canonical form")]),
    guide("demo.align_scan", "inspect", "Align a part and scan it",
          [("demo.align", "Align to the datum features"), ("demo.scan", "Scan the surface")]),
    guide("demo.check_flatness", "inspect", "Check a face for flatness", [("demo.fit_plane", "Fit a plane")]),
    guide("demo.compare_nominal", "inspect", "Compare a scan with nominal", [("demo.compare", "Compare")]),
    guide("demo.export_report", "inspect", "Export an inspection report", [("demo.export_report", "Export")]),
    guide("demo.measure_bore", "inspect", "Measure a bore", [("demo.fit_cylinder", "Fit a cylinder")]),
    guide("demo.unlisted", "inspect", "A guide no workflow names", [("demo.scan", "Scan")]),
]


def manifest(qdigest):
    return canon({"format": 1, "quantity_catalogue_digest": qdigest, "commands": COMMANDS,
                  "task_guides": sorted(GUIDES, key=lambda g: g["id"])})


MANIFEST_OK = manifest("sha256:" + "0" * 64)
MANIFEST_OLD = manifest("sha256:" + "1" * 64)  # an older export: different bytes, different digest
DIGEST_OK = sha(MANIFEST_OK)
DIGEST_OLD = sha(MANIFEST_OLD)
write("manifest-ok.json", MANIFEST_OK)
write("manifest-old.json", MANIFEST_OLD)

# Profile index as written by AUTO.MCP.SRV (only the fields the check reads).
write("profiles-ok.json", pretty({"manifest_digest": DIGEST_OK, "profiles": [{"name": "core"}, {"name": "inspect"}]}))

# ---------------------------------------------------------------- generated Skills


def skills(folder, digest_for):
    root = HERE / folder
    if root.exists():
        shutil.rmtree(root)
    for g in GUIDES:
        d = g["id"].replace(".", "-").replace("_", "-")
        text = (f"---\nname: {d}\ndescription: {g['title']}\nmanifest_digest: {digest_for(g['id'])}\n---\n\n"
                f"# {g['title']}\n\n{g['summary']}\n")
        write(f"{folder}/{d}/SKILL.md", text.encode("utf-8"))


skills("skills-ok", lambda gid: DIGEST_OK)
# One Skill left over from the older manifest (AC-3).
skills("skills-stale", lambda gid: DIGEST_OLD if gid == "demo.measure_bore" else DIGEST_OK)

# ---------------------------------------------------------------- recipes

RECIPES = {
    "demo.align_scan_basic": {"id": "demo.align_scan_basic", "title": "Basic align and scan",
                              "prompt": "Align the fixture part and scan its top face."},
    "demo.measure_bore_basic": {"id": "demo.measure_bore_basic", "title": "Basic bore",
                                "prompt": "Measure the bore of the fixture part."},
    "demo.measure_bore_deep": {"id": "demo.measure_bore_deep", "title": "Deep bore",
                               "prompt": "Measure the deep bore in three sections."},
}
recipes_dir = HERE / "recipes"
if recipes_dir.exists():
    shutil.rmtree(recipes_dir)
RECIPE_BYTES = {}
for rid, body in RECIPES.items():
    RECIPE_BYTES[rid] = pretty(body)
    write(f"recipes/{rid}.json", RECIPE_BYTES[rid])

# ---------------------------------------------------------------- catalogs


def row(profile, wid, title, gid, recipes=()):
    r = {"profile": profile, "workflow_id": wid, "title": title, "guide_id": gid}
    if recipes:
        r["recipes"] = list(recipes)
    return r


OK_ROWS = [
    row("core", "core.canon_text", "Canonicalise JSON", "core.canon_text"),
    row("inspect", "inspect.align_scan", "Align and scan", "demo.align_scan", ["demo.align_scan_basic"]),
    row("inspect", "inspect.measure_bore", "Measure a bore", "demo.measure_bore",
        ["demo.measure_bore_basic", "demo.measure_bore_deep"]),
    row("inspect", "inspect.compare_nominal", "Compare with nominal", "demo.compare_nominal"),
    row("inspect", "inspect.export_report", "Export a report", "demo.export_report"),
    row("inspect", "inspect.check_flatness", "Check flatness", "demo.check_flatness"),
]


def catalog(rows):
    return pretty({"format": "fid.agent-catalog", "version": 1, "workflows": rows})


write("catalog-ok.json", catalog(OK_ROWS))
# AC-2: one workflow names guide demo.ghost.
write("catalog-missing-guide.json",
      catalog(OK_ROWS[:5] + [row("inspect", "inspect.ghost", "Ghost", "demo.ghost")]))
# AC-4: the app profile `inspect` lists 4 workflows.
write("catalog-too-few.json", catalog(OK_ROWS[:5]))
# REQ-005: repeated workflow ID; profile absent from the profile index.
write("catalog-duplicate.json", catalog(OK_ROWS + [row("inspect", "inspect.align_scan", "Again", "demo.unlisted")]))
write("catalog-unknown-profile.json", catalog(OK_ROWS + [row("measure", "measure.scan", "Scan", "demo.unlisted")]))
# REQ-003: an `inspect` workflow naming a `core` guide.
write("catalog-profile-mismatch.json",
      catalog(OK_ROWS + [row("inspect", "inspect.canon", "Canon", "core.canon_text")]))

# ---------------------------------------------------------------- golden index (AC-1)

guides_by_id = {g["id"]: g for g in GUIDES}
index_rows = []
for r in OK_ROWS:
    full = {"profile": r["profile"], "workflow_id": r["workflow_id"], "title": r["title"],
            "guide_id": r["guide_id"], "recipes": r.get("recipes", [])}
    version = sha(canon({"guide": guides_by_id[r["guide_id"]],
                         "recipes": [{"id": rid, "sha256": sha(RECIPE_BYTES[rid])} for rid in full["recipes"]],
                         "row": full}))
    index_rows.append({"workflow_id": r["workflow_id"], "catalog_version": version})
index_rows.sort(key=lambda x: x["workflow_id"])
write("catalog-index.json", canon({"manifest_digest": DIGEST_OK, "rows": index_rows}))
print("manifest-ok", DIGEST_OK)
