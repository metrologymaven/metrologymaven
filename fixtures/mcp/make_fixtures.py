#!/usr/bin/env python3
"""Independent reference for AUTO.MCP.PROJ (class-based MCP tool projection).

Writes, from the spec's rules and never from the Rust code:

- fixtures/manifest/motion-request.json  (AC-2: cmm.request_run@1 request-only with a run-ticket effect)
- fixtures/mcp/calls.json                (AC-3/AC-4 commands demo.release_plan@1 W3, demo.set_offset@1 W2)
- fixtures/mcp/cases.json                (generation cases: manifest text and the expected tool list or error)
- fixtures/mcp/min-classes.tools.json    (AC-1 golden: mcp/mcp-tools.json as the generator writes it
                                          for fixtures/manifest/min-classes.json)

Stdlib only. `--check` regenerates in memory and fails if any committed file differs.

Pinned rules (mirrored by crates/fid-mcp, stated in the PR):
- tool kind: the stricter of the declared projection (human_only clamps it to request_only) and the
  class ceiling (R0 R1 I1 W1 W2 exposed; W3 X1 D1 request_only; M1 S1 A0 never), and never unless
  "mcp" is in the exposure; never = no tool, request_only = request-only tool, exposed = direct tool;
- name: domain_verb, plus _vN for major N >= 2; title: the registry title; description: the registry
  description, plus " " + REQUEST_ONLY_SENTENCE for a request-only tool (the sentence alone if the
  description is empty);
- input schema: {additionalProperties: false, properties, required (byte order), type: object};
  a parameter with x-quantity or x-unit is {value: string, unit: string} (type must be number) with
  x-quantity / x-unit copied and no default, type object, or [null, object] when declared number or null;
  otherwise {type: T}, {type: [T...]} for a union of JSON
  types, {} for no type or a "#..." $ref, plus "default" when declared; anything else is
  schema_unrepresentable;
- output schema: schema/envelope-v1.json without $id and $schema, properties.result = the result type
  mapped the same way;
- annotations: read_only_hint for R0 R1; request_only; destructive_hint = direct and not read-only;
- motion: an effect run_ticket.create|approve|release (ASCII case-insensitive, optional @suffix) on a
  listed tool is motion_tool_forbidden unless the command is cmm.request_hold@1 or R0;
- text: any key or string of a tool holding a forbidden code point (the CORE.BUS.POL.PROJ reason
  table, plus U+xFFFE/U+xFFFF) is forbidden_character;
- the first error in (name bytes, major) order wins; the file is
  {"_header": "<generated, do not edit; manifest sha256:..; generator fid-surface-gen 1>",
   "manifest_digest", "surface": "mcp", "tools"} in RFC 8785 form plus a final LF.
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
MCP = ROOT / "fixtures" / "mcp"
MAN = ROOT / "fixtures" / "manifest"
QCD = "sha256:" + "ab" * 32

REQUEST_ONLY_SENTENCE = "Calling this tool only files a request for a human to review; it performs nothing itself."
MOTION_EFFECTS = {"run_ticket.create", "run_ticket.approve", "run_ticket.release"}
MOTION_ALLOWLIST = {"cmm.request_hold@1"}
JSON_TYPES = {"array", "boolean", "integer", "null", "number", "object", "string"}
RANK = {"never": 0, "request_only": 1, "exposed": 2}
CEILING = {
    "R0": "exposed", "R1": "exposed", "I1": "exposed", "W1": "exposed", "W2": "exposed",
    "W3": "request_only", "X1": "request_only", "D1": "request_only",
    "M1": "never", "S1": "never", "A0": "never",
}
# CORE.BUS.POL.PROJ REASON_FORBIDDEN_RANGES (Unicode 16.0 Cc, Cf, Zl, Zp, noncharacters, tags).
FORBIDDEN = [
    (0x0000, 0x001F), (0x007F, 0x009F), (0x00AD, 0x00AD), (0x0600, 0x0605), (0x061C, 0x061C),
    (0x06DD, 0x06DD), (0x070F, 0x070F), (0x0890, 0x0891), (0x08E2, 0x08E2), (0x180E, 0x180E),
    (0x200B, 0x200F), (0x2028, 0x2028), (0x2029, 0x2029), (0x202A, 0x202E), (0x2060, 0x2064),
    (0x2066, 0x206F), (0xFDD0, 0xFDEF), (0xFEFF, 0xFEFF), (0xFFF9, 0xFFFB), (0x110BD, 0x110BD),
    (0x110CD, 0x110CD), (0x13430, 0x1343F), (0x1BCA0, 0x1BCA3), (0x1D173, 0x1D17A), (0xE0000, 0xE007F),
]


def canon(v):
    """RFC 8785 for the values used here: ASCII keys, small integers and short decimals, strings."""
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def definition(name, major, side_effect, declared, human_only=False, params=None, required=(), result=None,
               title=None, description=None, effects=(), exposure=("cli", "mcp", "python")):
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
        "exposure": list(exposure),
        "lifecycle": {"state": "active"},
        "human_only": human_only,
        "web": "available",
    }
    if required:
        d["params_schema"]["required"] = list(required)
    d["definition_digest"] = sha(canon(d))
    return d


def manifest(defs):
    defs = sorted(defs, key=lambda d: (d["id"].encode("utf-8"), d["major"]))
    return canon({"format": 1, "quantity_catalogue_digest": QCD, "commands": defs})


# ------------------------------------------------------------------ the reference emitter


class Refused(Exception):
    def __init__(self, code, command):
        super().__init__(code)
        self.code = code
        self.command = command


def forbidden(c):
    u = ord(c)
    return (u & 0xFFFE) == 0xFFFE or any(lo <= u <= hi for lo, hi in FORBIDDEN)


def first_forbidden(v):
    if isinstance(v, str):
        return next((c for c in v if forbidden(c)), None)
    if isinstance(v, list):
        return next((x for x in map(first_forbidden, v) if x is not None), None)
    if isinstance(v, dict):
        for k, x in v.items():
            c = first_forbidden(k)
            if c is None:
                c = first_forbidden(x)
            if c is not None:
                return c
    return None


def type_of(schema):
    t = schema.get("type")
    if isinstance(t, str):
        return t
    if isinstance(t, list):
        return "|".join(sorted(set(t)))
    if "$ref" in schema:
        return schema["$ref"]
    return "any"


def type_schema(ty):
    if ty == "any" or ty.startswith("#"):
        return {}
    parts = ty.split("|")
    if not all(p in JSON_TYPES for p in parts):
        return None
    return {"type": parts[0]} if len(parts) == 1 else {"type": parts}


def kind_of(d):
    declared = d["mcp_projection"]
    if d.get("human_only", False) and RANK[declared] > RANK["request_only"]:
        declared = "request_only"
    ceiling = CEILING[d["side_effect"]]
    p = declared if RANK[declared] <= RANK[ceiling] else ceiling
    if "mcp" not in d["exposure"]:
        return None
    return {"never": None, "request_only": "request_only", "exposed": "direct"}[p]


def envelope_output(result_schema):
    env = json.loads((ROOT / "schema" / "envelope-v1.json").read_bytes())
    del env["$id"]
    del env["$schema"]
    env["properties"]["result"] = result_schema
    return env


def tool_of(d):
    cid = f"{d['id']}@{d['major']}"
    kind = kind_of(d)
    if kind is None:
        return None
    for e in d["effects"]:
        base = e.split("@")[0].lower()
        if base in MOTION_EFFECTS and d["side_effect"] != "R0" and cid not in MOTION_ALLOWLIST:
            raise Refused("motion_tool_forbidden", cid)
    ps = d["params_schema"]
    req = set(ps.get("required", []))
    props = {}
    for pname, p in ps.get("properties", {}).items():
        ty = type_of(p)
        if "x-quantity" in p or "x-unit" in p:
            if ty not in ("number", "null|number"):
                raise Refused("schema_unrepresentable", cid)
            q = {
                "type": "object" if ty == "number" else ["null", "object"],
                "additionalProperties": False, "required": ["unit", "value"],
                "properties": {"unit": {"type": "string"}, "value": {"type": "string"}},
            }
            if "x-quantity" in p:
                q["x-quantity"] = p["x-quantity"]
            if "x-unit" in p:
                q["x-unit"] = p["x-unit"]
            props[pname] = q
            continue
        s = type_schema(ty)
        if s is None:
            raise Refused("schema_unrepresentable", cid)
        if "default" in p:
            s["default"] = p["default"]
        props[pname] = s
    result = type_schema(type_of(d["result_schema"]))
    if result is None:
        raise Refused("schema_unrepresentable", cid)
    domain, verb = d["id"].split(".")
    name = f"{domain}_{verb}" + (f"_v{d['major']}" if d["major"] >= 2 else "")
    if kind == "request_only":
        desc = REQUEST_ONLY_SENTENCE if d["description"] == "" else d["description"] + " " + REQUEST_ONLY_SENTENCE
    else:
        desc = d["description"]
    read_only = d["side_effect"] in ("R0", "R1")
    tool = {
        "annotations": {
            "destructive_hint": kind == "direct" and not read_only,
            "read_only_hint": read_only,
            "request_only": kind == "request_only",
        },
        "command": {"command_id": cid, "major": d["major"]},
        "description": desc,
        "input_schema": {
            "additionalProperties": False,
            "properties": props,
            "required": sorted(req, key=lambda k: k.encode("utf-8")),
            "type": "object",
        },
        "name": name,
        "output_schema": envelope_output(result),
        "title": d["title"],
    }
    if first_forbidden(tool) is not None:
        raise Refused("forbidden_character", cid)
    return tool


def reference_tools(manifest_bytes):
    doc = json.loads(manifest_bytes)
    defs = sorted(doc["commands"], key=lambda d: (d["id"].encode("utf-8"), d["major"]))
    return [t for t in map(tool_of, defs) if t is not None]


def tools_file(manifest_bytes):
    digest = sha(manifest_bytes)
    body = canon({"manifest_digest": digest, "surface": "mcp", "tools": reference_tools(manifest_bytes)})
    header = f"generated, do not edit; manifest {digest}; generator fid-surface-gen 1"
    assert body.startswith(b"{") and not body.startswith(b"{}")
    return b'{"_header":' + canon(header) + b"," + body[1:] + b"\n"


def expectation(manifest_bytes):
    try:
        tools = reference_tools(manifest_bytes)
    except Refused as e:
        return {"error": e.code, "command": e.command}
    return {"ok": [[t["name"], "request_only" if t["annotations"]["request_only"] else "direct"] for t in tools]}


# ------------------------------------------------------------------ fixtures


def motion_request():
    return manifest([
        definition("cmm.request_run", 1, "W3", "request_only", effects=("run_ticket.create",),
                   params={"program": {"type": "string"}}, required=("program",)),
        definition("cmm.request_hold", 1, "W1", "exposed", effects=("run_ticket.hold",)),
        definition("cmm.get_run", 1, "R0", "exposed"),
    ])


def calls():
    return manifest([
        definition("demo.release_plan", 1, "W3", "request_only", params={"plan": {"type": "string"}},
                   required=("plan",), description="Releases a measurement plan."),
        definition("demo.set_offset", 1, "W2", "exposed",
                   params={"offset": {"type": "number", "x-quantity": "length", "x-unit": "mm"}},
                   required=("offset",), description="Sets the probe offset."),
        definition("demo.show_plan", 1, "R0", "exposed", params={"plan": {"type": "string"}}),
        definition("demo.erase_disk", 1, "A0", "never"),
        definition("demo.move_axis", 1, "M1", "never"),
    ])


def cases():
    plain = lambda n, c, decl, **kw: definition(n, 1, c, decl, **kw)  # noqa: E731
    out = {
        # AC-2: the hold request alone passes.
        "hold-alone": manifest([plain("cmm.request_hold", "W1", "exposed", effects=("run_ticket.hold",))]),
        # The allowlist: the hold request and R0 queries may declare a run-ticket effect.
        "allowlist": manifest([
            plain("cmm.request_hold", "W1", "exposed", effects=("run_ticket.create",)),
            plain("cmm.get_ticket", "R0", "exposed", effects=("run_ticket.approve",)),
        ]),
        # Case and @suffix do not hide a motion effect; R1 is not exempt.
        "motion-suffix": manifest([plain("demo.queue_run", "X1", "request_only", effects=("Run_Ticket.Approve@1",))]),
        "motion-r1": manifest([plain("demo.plan_run", "R1", "exposed", effects=("run_ticket.release",))]),
        # A motion command that is never listed does not fail generation.
        "motion-unlisted": manifest([
            plain("cmm.start_motion", "M1", "never", effects=("run_ticket.release",)),
            plain("cmm.move_hidden", "W2", "exposed", effects=("run_ticket.create",), exposure=("cli",)),
        ]),
        # Exposure: a command not exposed on mcp gets no tool, whatever its class.
        "exposure": manifest([
            plain("demo.cli_only", "R0", "exposed", exposure=("cli", "python")),
            plain("demo.internal", "W1", "exposed", exposure=()),
            plain("demo.listed", "R0", "exposed"),
        ]),
        # human_only tightens W1 and R0 to request-only.
        "human-only": manifest([
            plain("demo.adopt_plan", "W2", "exposed", human_only=True),
            plain("demo.read_secret", "R0", "request_only", human_only=True),
        ]),
        # Text: bidi override, C0 control and a tag character are refused; µ and é are fine.
        "text-bidi": manifest([plain("demo.show_a", "R0", "exposed", description="Shows ‮A")]),
        "text-newline": manifest([plain("demo.show_b", "R0", "exposed", title="Two\nlines")]),
        "text-tag": manifest([plain("demo.show_c", "R0", "exposed", description="tag \U000E0041")]),
        "text-param": manifest([plain("demo.show_d", "R0", "exposed", params={"x​": {"type": "string"}})]),
        "text-ok": manifest([plain("demo.show_e", "R0", "exposed", description="Offset in µm, é")]),
        # Schema: a quantity that is not a number, an unknown type and an external $ref.
        "schema-quantity": manifest([plain("demo.set_q", "W1", "exposed",
                                           params={"q": {"type": "string", "x-quantity": "length"}})]),
        "schema-optional-quantity": manifest([plain("demo.set_o", "W1", "exposed",
                                                    params={"o": {"type": ["number", "null"], "x-quantity": "time",
                                                                  "x-unit": "s"}})]),
        "schema-type": manifest([plain("demo.set_t", "W1", "exposed", params={"t": {"type": "decimal"}})]),
        "schema-result": manifest([plain("demo.show_r", "R0", "exposed",
                                         result={"$ref": "https://example.invalid/r.json"})]),
        # The first error in command order wins.
        "first-error": manifest([
            plain("zeta.queue_run", "W3", "request_only", effects=("run_ticket.create",)),
            plain("alpha.set_t", "W1", "exposed", params={"t": {"type": "decimal"}}),
        ]),
    }
    return canon([{"name": k, "manifest": v.decode("utf-8"), "expect": expectation(v)}
                  for k, v in sorted(out.items())]) + b"\n"


def outputs():
    min_classes = (MAN / "min-classes.json").read_bytes()
    files = {
        MAN / "motion-request.json": motion_request(),
        MCP / "calls.json": calls(),
        MCP / "cases.json": cases(),
        MCP / "min-classes.tools.json": tools_file(min_classes),
    }
    assert expectation(files[MAN / "motion-request.json"]) == {"error": "motion_tool_forbidden",
                                                              "command": "cmm.request_run@1"}
    assert len(reference_tools(min_classes)) == 10
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
