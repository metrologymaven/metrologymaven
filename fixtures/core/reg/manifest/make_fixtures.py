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

Rules (NODE.md Design, pinned choices):
- each declaration in normal form: `human_only` false, `network` "none", `cli_bare` false
  when absent; `exposure` in surface order cli, mcp, python, gui, control; every other field
  as declared;
- definition_digest = SHA-256 of the RFC 8785 bytes of that normal form;
- commands sorted by id (byte order) then major;
- manifest = {"commands", "format": 1, "quantity_catalogue_digest"} in RFC 8785 form, where
  the catalogue digest is the `GEO.FRAME.UNITS` catalogue digest of
  ../def/schema/catalogue/geo_frame_units.sha256 (itself computed with hashlib).

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
    doc = json.loads(valid)
    doc["commands"][0]["title"] = doc["commands"][0]["title"] + " (tampered)"
    return {
        "valid.manifest.json": valid,
        "valid.digest": sha(valid) + "\n",
        "order.json": json.dumps({"decls": order}, indent=1, ensure_ascii=False) + "\n",
        "order.manifest.json": order_manifest,
        "order.digest": sha(order_manifest) + "\n",
        "tampered.json": jcs(doc),
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
