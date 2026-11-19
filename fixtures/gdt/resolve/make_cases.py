#!/usr/bin/env python3
"""Independent reference for GDT.RES.RESOLVE fixtures (stdlib only).

Reads the synthetic DMT table of GDT.RES.SCHEMA (`../dmt/synthetic_table_v1.json`) and the
resolver cases in this script, applies the leaf's rules as written in its NODE.md (derive the
key `dmt:<dialect>/<characteristic>/<sorted modifiers|none>`, exact lookup over every segment
but the row version, edition sets must intersect, no near-match fallback, two matches are
refused, profile row choices for rows the table lacks refused, status branch, variant
precedence override > profile > row default), and writes:

- `cases.json`: the resolver inputs, one per case;
- `expected/<case>.json`: the RFC 8785 canonical JSON the resolver must print for that case
  (`{"error": "<code>"}` for a refusal);
- `expected/get_row_flatness.json`: the canonical JSON of row `dmt:asme/flatness/none@1`
  (AC-7);
- `flatness_asme.json`: the inline characteristic `flat-1` of AC-1 (`fid.gdt-resolve-char/1`);
- `profile_position_axis.json`: an ASME defaults profile choosing variant `axis` for
  `dmt:asme/position/none@1` (AC-4 through the command layer).

The RFC 8785 (JCS) serializer here is this script's own, not the Rust code under test. It
covers objects, arrays, strings and booleans (a DMT table holds no numbers): members sorted by
UTF-16 code units; strings escape `"`, `\\` and control characters. With `--check` it rebuilds
every file in memory and fails if any committed file differs byte for byte.

Everything is synthetic placeholder data with no standards text; CC0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
TABLE = HERE.parent / "dmt" / "synthetic_table_v1.json"
TABLE_DIGEST = HERE.parent / "dmt" / "synthetic_table_v1.digest"
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def jcs_string(s):
    out = ['"']
    for ch in s:
        if ch in SHORT:
            out.append(SHORT[ch])
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def jcs(v):
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise ValueError(f"unsupported value {v!r}")


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate key {k!r}")
        out[k] = v
    return out


def load_table():
    t = json.loads(TABLE.read_text(encoding="utf-8"), object_pairs_hook=no_dupes)
    rows = sorted(t["rows"], key=lambda r: r["key"].encode("utf-8"))
    digest = sha(jcs({"rows": rows, "schema": t["schema"], "version": t["version"]}))
    # Cross-check against the GDT.RES.SCHEMA reference's own digest file.
    assert digest == TABLE_DIGEST.read_text(encoding="utf-8").strip(), "table digest disagrees"
    return rows, digest


ED = ["y14.5-2018"]
POS = "dmt:asme/position/none@1"

# name -> (dialect, characteristic, modifiers as given, editions, profile, override)
CASES = [
    # AC-1: one row matches; no override, no profile choice: the row default.
    ("flatness_asme", "asme", "flatness", [], ED, {}, None),
    # AC-2: no row for this modifier combination; no fallback to flatness/none.
    ("unknown_modifier", "asme", "flatness", ["lmc"], ED, {}, None),
    # AC-3: the unsupported row; the status branch wins over an override.
    ("unsupported_concentricity", "asme", "concentricity", [], ED, {}, None),
    ("unsupported_with_override", "asme", "concentricity", [], ED, {}, "median"),
    # AC-3: partial rows return their chain plus limitation codes (modifiers given unsorted).
    ("partial_perpendicularity", "asme", "perpendicularity", [], ED, {}, None),
    ("partial_position_cz_mmc", "asme", "position", ["mmc", "cz"], ED, {}, None),
    # AC-4: profile choice, then an override on top of it.
    ("position_profile_axis", "asme", "position", [], ED, {POS: "axis"}, None),
    ("position_override_surface", "asme", "position", [], ED, {POS: "axis"}, "surface"),
    ("position_row_default", "asme", "position", [], ED, {"dmt:asme/position/mmc@1": "axis"}, None),
    # AC-5: the override and the profile name a variant the row lacks.
    ("position_override_median", "asme", "position", [], ED, {}, "median"),
    ("position_profile_unknown", "asme", "position", [], ED, {POS: "variant-b"}, None),
    # No edition fallback: a characteristic under another edition does not match.
    ("wrong_edition", "asme", "flatness", [], ["y14.5-2009"], {}, None),
    ("extra_edition", "asme", "flatness", [], ["y14.5-2018", "y14.5.1-2019"], {}, None),
    # No dialect fallback: ISO flatness is its own (unsupported) row, ISO angularity has none.
    ("iso_flatness", "iso", "flatness", [], ["1101-2017"], {}, None),
    ("iso_angularity", "iso", "angularity", [], ["1101-2017"], {}, None),
    # A profile row choice for a row the table lacks is refused, even for another row's lookup.
    ("profile_row_unknown", "asme", "flatness", [], ED, {"dmt:asme/flatness/lmc@1": "default"}, None),
    ("profile_row_unknown_bad_key", "asme", "flatness", [], ED, {"dmt:not a key": "x"}, None),
    # Named (non-row) profile keys are not the resolver's.
    ("profile_named_key_ignored", "asme", "flatness", [], ED, {"unc.decision_rule_hint": "x"}, None),
    # A single-variant row still honours a profile choice that names its variant.
    ("size_profile_global", "asme", "size", [], ED, {"dmt:asme/size/none@1": "global"}, None),
]


def resolve(rows, digest, dialect, char, mods, editions, profile, override):
    keys = {r["key"] for r in rows}
    if any(k.startswith("dmt:") and k not in keys for k in profile):
        return {"error": "profile_row_unknown"}
    mods = sorted(mods, key=lambda m: m.encode("utf-8"))
    key = f"dmt:{dialect}/{char}/{'+'.join(mods) if mods else 'none'}"
    cands = [r for r in rows if r["key"].rsplit("@", 1)[0] == key and set(r["editions"]) & set(editions)]
    if not cands:
        return {"outcome": "UNRESOLVED", "key": key, "dmt_digest": digest}
    if len(cands) > 1:
        return {"error": "ambiguous_resolution"}
    row = cands[0]
    if row["status"] == "unsupported":
        return {"outcome": "UNSUPPORTED", "row": row["key"], "dmt_digest": digest}
    if override is not None:
        name, source = override, "override"
    elif row["key"] in profile:
        name, source = profile[row["key"]], "profile"
    else:
        name, source = row["default_variant"], "row_default"
    chains = [c for c in row["variants"] if c["variant"] == name]
    if not chains:
        return {"error": "unknown_variant"}
    chain = chains[0]
    return {
        "outcome": "RESOLVED",
        "row": row["key"],
        "row_status": row["status"],
        "variant": name,
        "variant_source": source,
        "chain": chain,
        "chain_digest": sha(jcs(chain)),
        "dmt_digest": digest,
        "limitations": row["limitations"],
    }


def pretty(v):
    return json.dumps(v, indent=2, ensure_ascii=False) + "\n"


def build():
    rows, digest = load_table()
    out = {}
    cases = []
    for name, dialect, char, mods, editions, profile, override in CASES:
        cases.append({
            "name": name, "dialect": dialect, "characteristic": char, "modifiers": mods,
            "editions": editions, "profile": profile, "override": override,
        })
        out[f"expected/{name}.json"] = jcs(resolve(rows, digest, dialect, char, mods, editions, profile, override))
    out["cases.json"] = pretty({"note": "Resolver inputs; written by make_cases.py; synthetic, CC0.", "cases": cases})
    flat = [r for r in rows if r["key"] == "dmt:asme/flatness/none@1"]
    assert len(flat) == 1
    out["expected/get_row_flatness.json"] = jcs(flat[0])
    out["flatness_asme.json"] = pretty({
        "schema": "fid.gdt-resolve-char/1", "char": "flat-1", "characteristic": "flatness",
        "modifiers": [], "override": None,
    })
    out["profile_position_axis.json"] = pretty({
        "schema": "fid.defaults-profile/1", "dialect": "asme", "edition": "y14.5-2018", "version": "2.0.0",
        "keys": {POS: "axis"},
    })
    return out


def main():
    check = "--check" in sys.argv[1:]
    failed = False
    for name, text in build().items():
        data = text.encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{name}: differs from make_cases.py output", file=sys.stderr)
                failed = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if failed:
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
