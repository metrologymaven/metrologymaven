#!/usr/bin/env python3
"""Independent reference for AUTO.SURF.PROJ (surface projection model and name derivation).

Writes the hand-specified manifests under fixtures/manifest/, the golden projection
fixtures/projection/min-classes.golden.json (and stricter.golden.json), and a seeded fuzz corpus
fixtures/projection/fuzz.json whose expected results come from this script's own
implementation of the spec rules (never from the Rust code).

Stdlib only. `--check` regenerates in memory and fails if any committed file differs.

Pinned choices (mirrored by crates/fid-surface, stated in the PR):
- the manifest is CORE.REG.MAN's object {format: 1, quantity_catalogue_digest, commands}, each
  command a CORE.REG.DEF.FIELD definition plus definition_digest = SHA-256 of the RFC 8785
  form of the definition without that field; the manifest bytes must be canonical;
- commands are sorted by name bytes, then numeric major; errors are checked per command in that
  order: name grammar, Python keyword, MCP length, projection, collisions (cli, mcp, python);
  definition-level errors (invalid command name, major > 99999) come first, in manifest order;
- a W1/W2 command declares `exposed` in the registry and projects to `named`; `human_only`
  tightens the expected projection to `request_only` (D-031);
- the projection is the stricter of the declaration and that class ceiling (the bus rule of
  CORE.BUS.POL.PROJ): a declaration looser than the ceiling is `projection_mismatch`, a
  stricter one projects to itself (`request_only`, or `never` as `none`);
- majors of one command share a CLI path, so CLI paths collide only across command names;
- the fuzz PRNG is splitmix64 with the seed below.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MAN = ROOT / "fixtures" / "manifest"
PROJ = ROOT / "fixtures" / "projection"

SEED = 0x5EED_A070_5F00_0001
FUZZ_CASES = 200
ZERO = "sha256:" + "0" * 64
QCD = "sha256:" + "ab" * 32

CLASSES = ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]
CLASS_PROJ = {
    "R0": "exposed", "R1": "exposed", "I1": "exposed",
    "W1": "named", "W2": "named",
    "W3": "request_only", "X1": "request_only", "D1": "request_only",
    "M1": "none", "S1": "none", "A0": "none",
}
VISIBILITY = ["exposed", "named", "request_only", "none"]  # most visible first
DECLARED = {"exposed": "exposed", "named": "exposed", "request_only": "request_only", "none": "never"}
PY_KEYWORDS = {
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else",
    "except", "finally", "for", "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not",
    "or", "pass", "raise", "return", "try", "while", "with", "yield",
}


def canon(v):
    """RFC 8785 for the values used here: ASCII keys, integers, strings, bools, null."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ------------------------------------------------------------------ manifest building


def definition(name, major, side_effect, declared, human_only=False, params=None, required=(), result=None,
               title=None, description=None, effects=()):
    d = {
        "id": name,
        "major": major,
        "title": title if title is not None else f"Fixture {name}@{major}",
        "description": description if description is not None else f"Fixture definition of class {side_effect}.",
        "params_schema": {"type": "object", "properties": params or {}},
        "result_schema": result if result is not None else {"type": "object"},
        "side_effect": side_effect,
        "mcp_projection": declared,
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "not_undoable",
        "preconditions": [],
        "effects": list(effects),
        "algorithms": [],
        "exposure": ["cli", "mcp", "python"],
        "lifecycle": {"state": "active"},
        "human_only": human_only,
        "web": "available",
    }
    if required:
        d["params_schema"]["required"] = list(required)
    d["definition_digest"] = sha(canon(d))
    return d


def manifest(defs):
    return canon({"format": 1, "quantity_catalogue_digest": QCD, "commands": defs})


# ------------------------------------------------------------------ the reference projection


def is_segment(s):
    return (
        len(s) > 0
        and "a" <= s[0] <= "z"
        and (("a" <= s[-1] <= "z") or ("0" <= s[-1] <= "9"))
        and all(("a" <= c <= "z") or ("0" <= c <= "9") or c == "_" for c in s)
    )


def is_command_name(s):
    edge = lambda c: ("a" <= c <= "z") or ("0" <= c <= "9")
    return len(s) > 0 and edge(s[0]) and edge(s[-1]) and all(edge(c) or c in "_." for c in s)


class Refused(Exception):
    def __init__(self, code, commands):
        super().__init__(code)
        self.code = code
        self.commands = commands


def type_of(schema):
    t = schema.get("type")
    if isinstance(t, str):
        return t
    if isinstance(t, list):
        return "|".join(sorted(set(t)))
    if "$ref" in schema:
        return schema["$ref"]
    return "any"


def reference_project(manifest_bytes):
    doc = json.loads(manifest_bytes)
    entries = []
    for d in doc["commands"]:
        if not is_command_name(d["id"]):
            raise Refused("bad_command_id", [d["id"]])
        if d["major"] > 99999:
            raise Refused("bad_command_id", [f"{d['id']}@{d['major']}"])
        entries.append(d)
    entries.sort(key=lambda d: (d["id"].encode("utf-8"), d["major"]))
    cli, mcp, py = {}, {}, {}
    out = []
    for d in entries:
        name, major = d["id"], d["major"]
        cid = f"{name}@{major}"
        parts = name.split(".")
        if len(parts) != 2 or not is_segment(parts[0]) or not is_segment(parts[1]):
            raise Refused("bad_command_id", [cid])
        domain, verb = parts
        if domain in PY_KEYWORDS or verb in PY_KEYWORDS:
            raise Refused("bad_name_chars", [cid])
        suffix = f"_v{major}" if major >= 2 else ""
        mcp_name = f"{domain}_{verb}{suffix}"
        if len(mcp_name) > 64:
            raise Refused("name_too_long", [cid])
        expected = CLASS_PROJ[d["side_effect"]]
        human_only = d.get("human_only", False)
        if human_only and VISIBILITY.index(expected) < VISIBILITY.index("request_only"):
            expected = "request_only"
        declared = d["mcp_projection"]
        if human_only and declared == "exposed":
            declared = "request_only"
        # The bus rule (CORE.BUS.POL.PROJ): the stricter of the declaration and the class
        # ceiling. Looser than the ceiling is refused (the registry refuses it too); stricter
        # projects to the declaration.
        rank = {"never": 0, "request_only": 1, "exposed": 2}
        if rank[declared] > rank[DECLARED[expected]]:
            raise Refused("projection_mismatch", [cid])
        if declared != DECLARED[expected]:
            expected = {"request_only": "request_only", "never": "none"}[declared]
        cli_path = ["fid", domain, verb.replace("_", "-")]
        key = " ".join(cli_path)
        if key in cli and cli[key][0] != name:
            raise Refused("name_collision", [cli[key][1], cid])
        cli.setdefault(key, (name, cid))
        for table, k in ((mcp, mcp_name), (py, f"fiducial.{domain}.{verb}{suffix}")):
            if k in table:
                raise Refused("name_collision", [table[k], cid])
            table[k] = cid
        ps = d["params_schema"]
        req = set(ps.get("required", []))
        params = []
        for pname in sorted(ps.get("properties", {}), key=lambda k: k.encode("utf-8")):
            p = ps["properties"][pname]
            spec = {
                "name": pname,
                "type": type_of(p),
                "quantity_kind": p.get("x-quantity"),
                "unit": p.get("x-unit"),
                "required": pname in req,
                "path_like": p.get("path_like", False),
            }
            if "default" in p:
                spec["default"] = p["default"]
            params.append(spec)
        out.append({
            "command_id": cid,
            "major": major,
            "cli_path": cli_path,
            "mcp_name": mcp_name,
            "py_module": f"fiducial.{domain}",
            "py_name": f"{verb}{suffix}",
            "mcp_projection": expected,
            "side_effect": d["side_effect"],
            "determinism": d["determinism"],
            "human_only": human_only,
            "params": params,
            "result": {"type": type_of(d["result_schema"])},
            "long_running": False,
            "dry_run": False,
            "help": {"title": d["title"], "description": d["description"]},
        })
    return canon({"manifest_digest": sha(manifest_bytes), "commands": out})


# ------------------------------------------------------------------ hand-specified fixtures

RICH_PARAMS = {
    "offset": {"type": "number", "x-quantity": "length", "x-unit": "mm", "default": 0},
    "mode": {"type": "string", "enum": ["fast", "exact"], "default": "exact"},
    "source": {"type": "string", "path_like": True},
    "frame": {"$ref": "#/$defs/frame_ref"},
    "note": {"type": ["string", "null"]},
    "angle": {"type": "number", "x-quantity": "angle", "x-unit": "deg", "default": 0.25},
}

MIN_CLASS_NAMES = {
    "R0": "project.list_features", "R1": "align.run_bestfit", "W1": "program.save_draft",
    "W2": "align.set_datum", "W3": "rpt.release_report", "X1": "egress.send", "I1": "sim.run_program",
    "D1": "probe.read_temperature", "M1": "motion.move_axis", "S1": "trust.sign_record", "A0": "plugin.install_pack",
}


def min_classes():
    defs = []
    for c in CLASSES:
        params = RICH_PARAMS if c == "W2" else {"tolerance": {"type": "number", "x-quantity": "length", "x-unit": "um"}}
        required = ("source", "offset") if c == "W2" else ()
        defs.append(definition(MIN_CLASS_NAMES[c], 1, c, DECLARED[CLASS_PROJ[c]], params=params, required=required,
                               description=f"Fixture definition of class {c} (µm, é)."))
    defs.append(definition("align.run_bestfit", 2, "R1", "exposed", result={"$ref": "#/$defs/alignment"}))
    # A human_only W2 adoption command: declared exposed, tightened to request_only (D-031).
    defs.append(definition("align.adopt_alignment", 1, "W2", "exposed", human_only=True))
    # Manifest order is (id, major) as CORE.REG.MAN exports it.
    defs.sort(key=lambda d: (d["id"].encode(), d["major"]))
    return manifest(defs)


def bad_projection():
    return manifest([
        definition("align.run_bestfit", 1, "R1", "exposed"),
        definition("motion.move_axis", 1, "M1", "exposed"),
    ])


def stricter():
    """Declarations stricter than the class ceiling, which the bus accepts (CORE.BUS.POL.PROJ:
    the stricter of the two wins), shaped like the bus's own request commands."""
    return manifest([
        # request.reject_request@1: W1, human_only, never (a person rejects; not on MCP).
        definition("request.reject_request", 1, "W1", "never", human_only=True),
        # request.file_request@1: W1, never (written only by the bus).
        definition("request.file_request", 1, "W1", "never"),
        definition("align.preview_fit", 1, "R0", "request_only"),
        definition("align.set_datum", 1, "W2", "request_only"),
        definition("rpt.release_report", 1, "W3", "never"),
    ])


def collision():
    return manifest([
        definition("demo.run_fit", 2, "R1", "exposed"),
        definition("demo.run_fit_v2", 1, "R1", "exposed"),
    ])


def long_name():
    # "demo_" + 60 = 65 characters.
    return manifest([definition("demo." + "x" * 60, 1, "R0", "exposed")])


# ------------------------------------------------------------------ AUTO.SURF.ROOTS

# Pinned rule (AUTO.SURF.ROOTS REQ-006, mirrored by crates/fid-surface/src/roots.rs): a command
# changes the roots if its declared effects list ROOTS_EFFECT or its class is A0.
ROOTS_EFFECT = "RootsChanged"
PRINCIPAL_KINDS = ["human", "agent", "script", "service", "device", "system"]


def roots_config():
    file_param = {"type": "string", "path_like": True, "x-extensions": ["stl"]}
    return manifest(sorted([
        definition("files.import_mesh", 1, "R1", "exposed", params={"source": file_param}, required=("source",)),
        definition("files.export_table", 1, "W1", "exposed", params={"target": file_param}, effects=("TableExported",)),
        definition("help.show_version", 1, "R0", "exposed"),
        # AC-5: an A0 command declaring the roots effect.
        definition("settings.set_roots", 1, "A0", "never", effects=(ROOTS_EFFECT,),
                   params={"roots": {"type": "array"}}),
        # The generic admin-setting command: A0, its effect names no key.
        definition("settings.set_admin_value", 1, "A0", "never", human_only=True,
                   effects=("settings.set_admin_value@1",)),
        # A W2 command (MCP-exposed by class) that declares the roots effect: hidden by metadata.
        definition("project.set_import_root", 1, "W2", "exposed", effects=("Other", ROOTS_EFFECT)),
    ], key=lambda d: (d["id"].encode(), d["major"])))


def roots_visibility(manifest_bytes):
    doc = json.loads(manifest_bytes)
    ids = sorted((d["id"].encode("utf-8"), d["major"], f"{d['id']}@{d['major']}") for d in doc["commands"])
    root_config = [cid for _, _, cid in ids
                   if any(d["id"] + "@" + str(d["major"]) == cid and
                          (d["side_effect"] == "A0" or ROOTS_EFFECT in d["effects"]) for d in doc["commands"])]
    visible = {k: [cid for _, _, cid in ids if k == "human" or cid not in root_config] for k in PRINCIPAL_KINDS}
    return canon({"root_config": root_config, "visible": visible}) + b"\n"


# ------------------------------------------------------------------ fuzz corpus


class SplitMix64:
    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n

    def pick(self, xs):
        return xs[self.below(len(xs))]


LETTERS = "abcdefghijklmnopqrstuvwxyz"
ALNUM_ = LETTERS + "0123456789_"
ODD_SEGMENTS = ["import", "class", "match", "type", "x_", "_x", "1x", "x__y", "a", "z9", "lambda", "case"]
ODD_NAMES = ["a..b", "a.b.c", "nodot", "a._b", "a_.b", "9a.b", "a.9b", "A.b", "-a.b", "a.b-c", ".a.b"]


def segment(rng, length):
    if length <= 1:
        return rng.pick(LETTERS)
    mid = "".join(rng.pick(ALNUM_) for _ in range(length - 2))
    return rng.pick(LETTERS) + mid + rng.pick(LETTERS + "0123456789")


def fuzz_name(rng):
    r = rng.below(40)
    if r == 0:
        return rng.pick(ODD_NAMES)
    if r == 1:
        return rng.pick(ODD_SEGMENTS) + "." + segment(rng, 1 + rng.below(6))
    if r == 2:
        return segment(rng, 1 + rng.below(6)) + "." + rng.pick(ODD_SEGMENTS)
    domain = rng.pick(["align", "demo", "gdt", "io", "a", "geo_dist", "x9"])
    if r < 8:
        # Lengths around the 64-character MCP boundary (with and without a suffix).
        verb_len = 64 - len(domain) - 1 + rng.pick([-4, -3, -2, -1, 0, 1, 2])
        return domain + "." + segment(rng, max(1, verb_len))
    return domain + "." + segment(rng, 1 + rng.below(12))


def fuzz_case(rng):
    n = 1 + rng.below(5)
    specs = []
    for _ in range(n):
        r = rng.below(10)
        if specs and r < 3:
            # Reuse a previous name: same command at another major, or the `_vN` collision form.
            base_name, base_major = specs[rng.below(len(specs))][:2]
            if r == 0:
                name, major = base_name, rng.pick([0, 1, 2, 3, base_major])
            elif r == 1:
                name, major = base_name + "_v" + str(2 + rng.below(3)), 1
            else:
                name, major = base_name, base_major + 1
        else:
            name, major = fuzz_name(rng), rng.pick([0, 1, 1, 1, 1, 1, 2, 2, 3, 3, 10, 10, 99999, 100000])
        c = rng.pick(CLASSES)
        human_only = rng.below(7) == 0
        expected = CLASS_PROJ[c]
        if human_only and VISIBILITY.index(expected) < VISIBILITY.index("request_only"):
            expected = "request_only"
        declared = DECLARED[expected]
        if human_only and expected == "request_only" and rng.below(2) == 0:
            declared = "exposed"  # effective projection is still request_only
        if rng.below(12) == 0:
            declared = rng.pick(["exposed", "request_only", "never"])
        specs.append((name, major, c, declared, human_only))
    defs = []
    for name, major, c, declared, human_only in specs:
        params = {}
        for _ in range(rng.below(4)):
            pname = segment(rng, 1 + rng.below(8))
            p = {"type": rng.pick(["number", "string", "boolean", "integer"])}
            if p["type"] == "number" and rng.below(2):
                p["x-quantity"], p["x-unit"] = rng.pick([("length", "mm"), ("angle", "rad"), ("length", "in")])
            if rng.below(3) == 0:
                p["default"] = rng.pick([0, 1, -3, "x", True, None, [1, 2]])
            if rng.below(5) == 0:
                p["path_like"] = True
            params[pname] = p
        required = tuple(k for k in sorted(params) if rng.below(2))
        defs.append(definition(name, major, c, declared, human_only=human_only, params=params, required=required))
    # Manifest array order is shuffled: the projection sorts on its own.
    for i in range(len(defs) - 1, 0, -1):
        j = rng.below(i + 1)
        defs[i], defs[j] = defs[j], defs[i]
    return manifest(defs)


def expectation(m):
    try:
        return {"ok": sha(reference_project(m))}
    except Refused as e:
        return {"error": e.code, "commands": e.commands}


def fuzz_corpus():
    rng = SplitMix64(SEED)
    cases = []
    for i in range(FUZZ_CASES):
        m = fuzz_case(rng)
        cases.append({"case": i, "manifest": m.decode("utf-8"), "expect": expectation(m)})
    return canon({"seed": f"{SEED:#x}", "cases": cases}) + b"\n"


def outputs():
    files = {
        MAN / "min-classes.json": min_classes(),
        MAN / "bad-projection.json": bad_projection(),
        MAN / "collision.json": collision(),
        MAN / "stricter.json": stricter(),
        MAN / "long-name.json": long_name(),
        MAN / "roots-config.json": roots_config(),
    }
    reference_project(files[MAN / "roots-config.json"])  # must project cleanly
    files[PROJ / "roots-config.visible.json"] = roots_visibility(files[MAN / "roots-config.json"])
    files[PROJ / "min-classes.golden.json"] = reference_project(files[MAN / "min-classes.json"])
    files[PROJ / "stricter.golden.json"] = reference_project(files[MAN / "stricter.json"])
    expected = {}
    for p in (MAN / "bad-projection.json", MAN / "collision.json", MAN / "long-name.json"):
        expected[p.name] = expectation(files[p])
    files[PROJ / "errors.expected.json"] = canon(expected) + b"\n"
    files[PROJ / "fuzz.json"] = fuzz_corpus()
    return files


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, data in outputs().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path.relative_to(ROOT)}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check and bad:
        sys.exit(1)
    if not check:
        print("wrote fixtures")


if __name__ == "__main__":
    main()
