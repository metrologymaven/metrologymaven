#!/usr/bin/env python3
"""Independent fixture generator and oracle for spec node ALN.DATUM.SYS.GATE (Metrology Maven).

CC0 synthetic test data. Every DMT row is marked `synthetic: true` (test-only placeholder,
not an interpretation of any standard); standards are cited by designator only and no
standards text appears anywhere.

Writes (or, with --check, regenerates and compares byte for byte):
  test_dmt_supported.json              Test-only `fid.dmt-table/1` with ASME primary/secondary/
                                       tertiary planar RMB datum rows (all `supported`).
  test_dmt_secondary_unsupported.json  Same table with the secondary datum row marked
                                       `unsupported` (for AC-3).
  gate_asme.json                       AC-1 input (datums A plane, B plane, C plane at RMB;
                                       ASME Y14.5-2018; ASME defaults profile) plus
                                       independently computed expected row IDs, chain digests,
                                       DMT digest, and `combined_digest` (SHA-256 of RFC 8785
                                       canonical JSON of the ordered per-datum chain digests).

Run (stdlib only, Python 3.9+):
  python fixtures/aln/datum/sys/make_gate_fixtures.py           # rewrite
  python fixtures/aln/datum/sys/make_gate_fixtures.py --check   # fail if stale
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def jcs_string(s: str) -> str:
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


def jcs(v) -> str:
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


def sha(text: str) -> str:
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def make_plane_datum_row(precedence: str, clause_num: str, supported: bool) -> dict:
    char = f"datum_plane_{precedence}"
    key = f"dmt:asme/{char}/rmb@1"
    if supported:
        variants = [
            {
                "variant": "lsq_plane",
                "steps": [
                    {
                        "kind": "partition",
                        "alg": "syn.partition.whole_feature@0.1.0",
                        "params": {},
                    },
                    {
                        "kind": "association",
                        "alg": f"syn.assoc.lsq_plane_{precedence}@0.1.0",
                        "params": {"constrained": True},
                    },
                ],
            },
            {
                "variant": "tangent_plane",
                "steps": [
                    {
                        "kind": "partition",
                        "alg": "syn.partition.whole_feature@0.1.0",
                        "params": {},
                    },
                    {
                        "kind": "association",
                        "alg": f"syn.assoc.datum_plane_{precedence}@0.1.0",
                        "params": {"constrained": True},
                    },
                    {
                        "kind": "construction",
                        "alg": "syn.construct.datum_plane@0.1.0",
                        "params": {},
                    },
                ],
            },
        ]
        default_variant = "tangent_plane"
        status = "supported"
        limitations = []
    else:
        variants = [{"variant": "placeholder", "steps": []}]
        default_variant = "placeholder"
        status = "unsupported"
        limitations = ["test_only_unsupported"]

    return {
        "key": key,
        "dialect": "asme",
        "editions": ["y14.5-2018"],
        "characteristic": char,
        "modifiers": ["rmb"],
        "clauses": [{"designator": "asme-y14.5", "edition": "2018", "clause": clause_num}],
        "variants": variants,
        "default_variant": default_variant,
        "actual_value": f"av.synthetic.{char}@1",
        "decision_rule_hint": "dr.synthetic.default@1",
        "ors_cases": [],
        "status": status,
        "limitations": limitations,
        "review": {"state": "draft"},
        "notes": "Test-only synthetic datum-gate row (test_only); not an interpretation of any standard.",
        "synthetic": True,
    }


def make_table(rows: list[dict]) -> tuple[dict, str]:
    sorted_rows = sorted(rows, key=lambda r: r["key"].encode("utf-8"))
    table = {
        "schema": "fid.dmt-table/1",
        "version": "1.0.0",
        "rows": sorted_rows,
    }
    digest = sha(jcs(table))
    return table, digest


def chain_digest(variant_obj: dict) -> str:
    return sha(jcs({"steps": variant_obj["steps"], "variant": variant_obj["variant"]}))


def default_variant_obj(row: dict) -> dict:
    for v in row["variants"]:
        if v["variant"] == row["default_variant"]:
            return v
    raise ValueError(f"default_variant not found in {row['key']}")


def build_fixtures() -> dict[str, bytes]:
    row_p = make_plane_datum_row("primary", "91.1", True)
    row_s = make_plane_datum_row("secondary", "91.2", True)
    row_t = make_plane_datum_row("tertiary", "91.3", True)

    supported_table, supported_digest = make_table([row_p, row_s, row_t])

    row_s_unsup = make_plane_datum_row("secondary", "91.2", False)
    unsup_table, unsup_digest = make_table([row_p, row_s_unsup, row_t])

    # AC-1: datums A (primary plane), B (secondary plane), C (tertiary plane) at RMB.
    # Profile chooses row defaults (no per-row override in profile_choices).
    chain_digests = [
        chain_digest(default_variant_obj(row_p)),
        chain_digest(default_variant_obj(row_s)),
        chain_digest(default_variant_obj(row_t)),
    ]
    combined_digest = sha(jcs(chain_digests))

    gate_asme = {
        "schema": "fid.aln-datum-gate-fixture/1",
        "dmt_table": "test_dmt_supported.json",
        "dmt_table_secondary_unsupported": "test_dmt_secondary_unsupported.json",
        "context": {
            "dialect": "asme",
            "edition": "y14.5-2018",
            "defaults_profile": "asme-y14.5-2018-default",
            "profile_choices": {},
        },
        "datums": [
            {"label": "A", "feature_type": "plane", "modifiers": ["rmb"]},
            {"label": "B", "feature_type": "plane", "modifiers": ["rmb"]},
            {"label": "C", "feature_type": "plane", "modifiers": ["rmb"]},
        ],
        "expected": {
            "dmt_digest": supported_digest,
            "secondary_unsupported_dmt_digest": unsup_digest,
            "row_ids": [row_p["key"], row_s["key"], row_t["key"]],
            "variants": ["tangent_plane", "tangent_plane", "tangent_plane"],
            "variant_sources": ["row_default", "row_default", "row_default"],
            "chain_digests": chain_digests,
            "combined_digest": combined_digest,
        },
    }

    return {
        "test_dmt_supported.json": (json.dumps(supported_table, indent=2) + "\n").encode("utf-8"),
        "test_dmt_secondary_unsupported.json": (json.dumps(unsup_table, indent=2) + "\n").encode("utf-8"),
        "gate_asme.json": (json.dumps(gate_asme, indent=2) + "\n").encode("utf-8"),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    extra = [a for a in argv[1:] if a != "--check"]
    if extra:
        sys.stderr.write(f"usage: {argv[0]} [--check]\n")
        return 2

    files = build_fixtures()
    if check:
        stale = []
        for name, want in sorted(files.items()):
            path = HERE / name
            if not path.is_file() or path.read_bytes() != want:
                stale.append(name)
        if stale:
            sys.stderr.write(f"stale fixtures in {HERE}: {', '.join(stale)}\n")
            return 1
        return 0

    for name, content in sorted(files.items()):
        (HERE / name).write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
