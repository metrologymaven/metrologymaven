#!/usr/bin/env python3
"""Expected outputs for CORE.STO.SET (settings model, scopes and settings commands).

Independent of the Rust code. From the hand-written inputs in this folder (`decls.json`,
`user_ac2.json`, `user_bad_length.json`) and the leaf's rules, this script writes:

- `golden/<command>.params.json`, `golden/<command>.result.json`: the four commands' schemas,
  written out by hand from the declaration-macro rules (fields in declaration order,
  `additionalProperties: false`, sorted `required` without `Option` fields, unit enums as
  snake-case strings in declaration order, value keywords on the field's own schema);
- `expected/*.json`: the settings manifest, listings (AC-1, AC-2, AC-3), command results,
  journal entries and the user file after a change (AC-6), from a small reference resolver
  (project, user, machine, default; a value failing its schema is skipped with a warning).

Every expected result is validated with `jsonschema` against its golden result schema, and the
example parameters against the golden parameter schemas, so the published schemas accept the
real results (reviewer rule). Output is RFC 8785 canonical JSON plus a newline. `--check`
reports stale files instead of writing them. Needs `jsonschema` (CI installs it).
"""

import json
import pathlib
import sys

import jsonschema

HERE = pathlib.Path(__file__).resolve().parent
KEY_PATTERN = "^[a-z][a-z0-9_]*(\\.[a-z][a-z0-9_]*)+$"
SCOPES = ["project", "user", "machine"]

# ------------------------------------------------------------------ golden schemas

SCOPE = {"type": "string", "enum": ["project", "user", "machine"]}
SOURCE = {"type": "string", "enum": ["project", "user", "machine", "default"]}
VALUE = {"description": "A JSON value; checked against the declared schema of the key."}
KEY = {"type": "string", "pattern": KEY_PATTERN, "maxLength": 128}


def obj(properties, required):
    o = {"type": "object", "additionalProperties": False, "properties": properties}
    if required:
        o["required"] = sorted(required)
    return o


LISTED = obj(
    {
        "key": KEY,
        "value": VALUE,
        "source_scope": SOURCE,
        "default": VALUE,
        "allowed_scopes": {"type": "array", "items": SCOPE, "minItems": 1, "maxItems": 3},
        "admin": {"type": "boolean"},
    },
    ["key", "value", "source_scope", "default", "allowed_scopes", "admin"],
)
SET_PARAMS = obj({"key": KEY, "scope": SCOPE, "value": VALUE}, ["key", "scope", "value"])
SET_RESULT = obj({"key": KEY, "scope": SCOPE, "value": VALUE, "previous": VALUE}, ["key", "scope", "value"])

GOLDEN = {
    "settings.list_values@1": {
        "params": obj(
            {
                "prefix": {"type": "string", "description": "Only keys starting with this text.", "maxLength": 128},
                "scope": dict(SCOPE, description="Only keys that allow this scope."),
            },
            [],
        ),
        "result": {"type": "array", "items": LISTED},
    },
    "settings.set_value@1": {"params": SET_PARAMS, "result": SET_RESULT},
    "settings.reset_value@1": {
        "params": obj({"key": KEY, "scope": SCOPE}, ["key", "scope"]),
        "result": obj({"key": KEY, "scope": SCOPE, "previous": VALUE}, ["key", "scope"]),
    },
    "settings.set_admin_value@1": {"params": SET_PARAMS, "result": SET_RESULT},
}

# ------------------------------------------------------------------ reference resolver


def load(name):
    with open(HERE / name, encoding="utf-8") as f:
        return json.load(f, object_pairs_hook=no_duplicates)


def no_duplicates(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate key {k}")
        out[k] = v
    return out


def is_number(v):
    return isinstance(v, (int, float)) and not isinstance(v, bool)


def valid(schema, v):
    """The keywords the fixture schemas use (type, minimum, maximum, items, lengths, uniqueItems)."""
    t = schema.get("type")
    checks = {
        "number": is_number(v),
        "boolean": isinstance(v, bool),
        "string": isinstance(v, str),
        "array": isinstance(v, list),
    }
    if t is not None and not checks[t]:
        return False
    if is_number(v):
        if "minimum" in schema and v < schema["minimum"]:
            return False
        if "maximum" in schema and v > schema["maximum"]:
            return False
    if isinstance(v, str):
        if "minLength" in schema and len(v) < schema["minLength"]:
            return False
        if "maxLength" in schema and len(v) > schema["maxLength"]:
            return False
    if isinstance(v, list):
        if "maxItems" in schema and len(v) > schema["maxItems"]:
            return False
        if schema.get("uniqueItems") and len({canonical(x) for x in v}) != len(v):
            return False
        if "items" in schema and not all(valid(schema["items"], x) for x in v):
            return False
    return True


def listing(decls, stores):
    """`stores` maps scope -> {key: value}; returns (rows, warnings)."""
    rows, warnings = [], []
    for d in sorted(decls, key=lambda d: d["key"]):
        value, source = d["default"], "default"
        for scope in SCOPES:
            if d["key"] not in stores.get(scope, {}):
                continue
            v = stores[scope][d["key"]]
            if scope not in d["scopes"]:
                warnings.append({"key": d["key"], "reason": "scope_not_allowed", "scope": scope})
            elif not valid(d["schema"], v):
                warnings.append({"key": d["key"], "reason": "invalid_value", "scope": scope})
            elif source == "default":
                value, source = v, scope
        rows.append(
            {
                "admin": d["admin"],
                "allowed_scopes": [s for s in SCOPES if s in d["scopes"]],
                "default": d["default"],
                "key": d["key"],
                "source_scope": source,
                "value": value,
            }
        )
    return rows, warnings


# ------------------------------------------------------------------ outputs


def canonical(value):
    # RFC 8785 for this data: ASCII keys sorted, small integers and short decimals, no whitespace.
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def outputs():
    decls = load("decls.json")["settings"]
    user_ac2 = load("user_ac2.json")["values"]
    user_bad = load("user_bad_length.json")["values"]
    for d in decls:
        assert valid(d["schema"], d["default"]), d["key"]

    out = {}
    for cmd, kinds in GOLDEN.items():
        for kind, schema in kinds.items():
            out[f"golden/{cmd}.{kind}.json"] = schema

    out["expected/manifest.json"] = {
        "format": "fid-settings-manifest/1",
        "settings": [
            dict(d, scopes=[s for s in SCOPES if s in d["scopes"]]) for d in sorted(decls, key=lambda d: d["key"])
        ],
    }
    fresh, w = listing(decls, {})
    assert w == []
    out["expected/list_fresh.json"] = fresh
    in_project, w = listing(decls, {"project": {"probe.approach_distance": 0.2}, "user": user_ac2})
    assert w == []
    out["expected/list_ac2_project.json"] = in_project
    outside, w = listing(decls, {"user": user_ac2})
    assert w == []
    out["expected/list_ac2_outside.json"] = outside
    bad, w = listing(decls, {"user": user_bad})
    out["expected/list_ac3.json"] = bad
    out["expected/warnings_ac3.json"] = w

    # AC-6: a project-scope set on branch main by a human, then a user-scope set by a script.
    out["expected/set_project_result.json"] = {"key": "probe.approach_distance", "scope": "project", "value": 0.2}
    out["expected/entry_project.json"] = {
        "branch": "main",
        "command": "settings.set_value@1",
        "key": "probe.approach_distance",
        "principal": "human",
        "scope": "project",
        "value": 0.2,
    }
    out["expected/set_user_result.json"] = {"key": "probe.approach_distance", "scope": "user", "value": 0.5}
    out["expected/entry_user.json"] = {
        "command": "settings.set_value@1",
        "key": "probe.approach_distance",
        "principal": "script",
        "scope": "user",
        "value": 0.5,
    }
    out["expected/user_after_set.json"] = {"format": "fid-settings/1", "values": {"probe.approach_distance": 0.5}}
    # A reset of the user value set above.
    out["expected/reset_user_result.json"] = {"key": "probe.approach_distance", "previous": 0.5, "scope": "user"}
    # AC-4: the admin key set by a human through the A0 command.
    out["expected/set_admin_result.json"] = {
        "key": "net.allowed_roots",
        "scope": "machine",
        "value": ["/data/exports"],
    }
    out["expected/entry_admin.json"] = {
        "command": "settings.set_admin_value@1",
        "key": "net.allowed_roots",
        "principal": "human",
        "scope": "machine",
        "value": ["/data/exports"],
    }
    return out


def validate_against_schemas(out):
    """Published result schemas accept the real results; parameter schemas the real inputs."""
    result = lambda cmd: out[f"golden/{cmd}.result.json"]  # noqa: E731
    params = lambda cmd: out[f"golden/{cmd}.params.json"]  # noqa: E731
    for name in ["list_fresh", "list_ac2_project", "list_ac2_outside", "list_ac3"]:
        jsonschema.validate(out[f"expected/{name}.json"], result("settings.list_values@1"))
    for name in ["set_project_result", "set_user_result"]:
        jsonschema.validate(out[f"expected/{name}.json"], result("settings.set_value@1"))
    jsonschema.validate(out["expected/reset_user_result.json"], result("settings.reset_value@1"))
    jsonschema.validate(out["expected/set_admin_result.json"], result("settings.set_admin_value@1"))
    for e in ["entry_project", "entry_user", "entry_admin"]:
        entry = out[f"expected/{e}.json"]
        cmd = entry["command"]
        jsonschema.validate({k: entry[k] for k in ["key", "scope", "value"]}, params(cmd))
    jsonschema.validate({}, params("settings.list_values@1"))
    jsonschema.validate({"prefix": "probe.", "scope": "user"}, params("settings.list_values@1"))
    jsonschema.validate({"key": "ui.show_tips", "scope": "user"}, params("settings.reset_value@1"))
    # The parameter schemas refuse what they must.
    bad = [
        ({"key": "UI.tips", "scope": "user", "value": 1}, "settings.set_value@1"),
        ({"key": "ui.show_tips", "scope": "site", "value": 1}, "settings.set_value@1"),
        ({"key": "ui.show_tips", "scope": "user"}, "settings.set_value@1"),
        ({"key": "ui.show_tips", "scope": "user", "value": 1, "extra": 1}, "settings.set_value@1"),
    ]
    for doc, cmd in bad:
        try:
            jsonschema.validate(doc, params(cmd))
        except jsonschema.ValidationError:
            continue
        raise SystemExit(f"{cmd} params schema accepted {doc}")
    for kinds in GOLDEN.values():
        for schema in kinds.values():
            jsonschema.Draft202012Validator.check_schema(schema)


def main():
    check = sys.argv[1:] == ["--check"]
    out = outputs()
    validate_against_schemas(out)
    stale = []
    for rel, value in sorted(out.items()):
        path = HERE / rel
        text = canonical(value) + "\n"
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
