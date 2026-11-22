#!/usr/bin/env python3
"""Independent stdlib reference generator for UNC.MODEL.CONTRIB.CONTRACT fixtures
(CC-BY-4.0).

Computes canonical RFC 8785 JSON and SHA-256 digests independently of the Rust
implementation using Python's `struct`, `json`, and `hashlib`.

Usage:
    python3 fixtures/unc/model/contrib/make_fixtures.py
    python3 fixtures/unc/model/contrib/make_fixtures.py --check
"""

from __future__ import annotations

import hashlib
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def f64_hex(val: float) -> str:
    if val == 0.0:
        val = 0.0
    return struct.pack(">d", val).hex()


def canon(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha256_bytes(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def sha256_label(label: str) -> str:
    return sha256_bytes(label.encode("ascii"))


def make_operator(op_id: str, alg_id: str, alg_ver: str, params: dict, ports_in: list, ports_out: list) -> dict:
    return {
        "algorithm_id": alg_id,
        "algorithm_version": alg_ver,
        "id": op_id,
        "params": params,
        "params_digest": sha256_bytes(canon(params)),
        "ports_in": ports_in,
        "ports_out": ports_out,
    }


def canonicalize_model(model: dict) -> dict:
    inputs = sorted(model["inputs"], key=lambda n: n["id"])
    operators = sorted(model["operators"], key=lambda n: n["id"])
    outputs = sorted(model["outputs"], key=lambda n: n["id"])
    edges = sorted(
        model["edges"],
        key=lambda e: (e["to"]["node"], e["to"]["port"], e["from"]["node"], e["from"]["port"]),
    )
    return {
        "correlations": model.get("correlations", []),
        "edges": edges,
        "inputs": inputs,
        "name": model["name"],
        "operators": operators,
        "outputs": outputs,
        "schema": "uncertainty-model/v1",
        "semver": model["semver"],
    }


def make_contributor(
    name: str,
    semver: str,
    params: list[dict],
    template: dict,
    applicability: list[str],
    references: list[dict],
) -> dict:
    norm_params = []
    for p in sorted(params, key=lambda x: x["name"]):
        norm_params.append(
            {
                "max": f64_hex(p["max"]),
                "min": f64_hex(p["min"]),
                "name": p["name"],
                "source_kinds": sorted(set(p["source_kinds"])),
                "unit": p["unit"],
            }
        )
    norm_refs = sorted(
        ({"clause": r["clause"], "designator": r["designator"]} for r in references),
        key=lambda r: (r["designator"], r["clause"]),
    )
    body = {
        "applicability": sorted(set(applicability)),
        "id": f"uc:{name}@{semver}",
        "params": norm_params,
        "references": norm_refs,
        "schema": "contributor/v1",
        "semver": semver,
        "template": canonicalize_model(template),
    }
    digest = sha256_bytes(canon(body))
    full = dict(body)
    full["digest"] = digest
    return full


def example_template(alg_id: str = "unc.op.scale", alg_ver: str = "1.0.0") -> dict:
    return {
        "name": "example-gain",
        "semver": "1.0.0",
        "inputs": [
            {
                "id": "length_in",
                "quantity_kind": "length",
                "unit": "si:m",
                "distribution": {
                    "kind": "normal",
                    "params": {"mu": f64_hex(0.1), "sigma": f64_hex(1e-5)},
                },
            },
            {
                "id": "gain",
                "quantity_kind": "ratio",
                "unit": "si:1",
                "distribution": {
                    "kind": "normal",
                    "params": {"mu": f64_hex(1.0), "sigma": f64_hex(1e-4)},
                },
            },
        ],
        "operators": [
            make_operator(
                "scale_op",
                alg_id,
                alg_ver,
                {"factor": "$param:gain"},
                [
                    {"arity": "1", "name": "x", "quantity_kind": "length", "unit": "si:m"},
                    {"arity": "1", "name": "k", "quantity_kind": "ratio", "unit": "si:1"},
                ],
                [{"name": "y", "quantity_kind": "length", "unit": "si:m"}],
            )
        ],
        "outputs": [{"id": "scaled_length", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [
            {"from": {"node": "length_in", "port": "value"}, "to": {"node": "scale_op", "port": "x"}},
            {"from": {"node": "gain", "port": "value"}, "to": {"node": "scale_op", "port": "k"}},
            {"from": {"node": "scale_op", "port": "y"}, "to": {"node": "scaled_length", "port": "value"}},
        ],
        "correlations": [],
    }


def make_alpha_offset() -> dict:
    tmpl = {
        "name": "alpha-offset",
        "semver": "1.0.0",
        "inputs": [
            {
                "id": "offset",
                "quantity_kind": "length",
                "unit": "si:m",
                "distribution": {
                    "kind": "normal",
                    "params": {"mu": f64_hex(0.0), "sigma": f64_hex(2e-6)},
                },
            }
        ],
        "operators": [
            make_operator(
                "pass_op",
                "unc.op.scale",
                "1.0.0",
                {"factor": f64_hex(1.0)},
                [{"arity": "1", "name": "x", "quantity_kind": "length", "unit": "si:m"}],
                [{"name": "y", "quantity_kind": "length", "unit": "si:m"}],
            )
        ],
        "outputs": [{"id": "out_len", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [
            {"from": {"node": "offset", "port": "value"}, "to": {"node": "pass_op", "port": "x"}},
            {"from": {"node": "pass_op", "port": "y"}, "to": {"node": "out_len", "port": "value"}},
        ],
        "correlations": [],
    }
    return make_contributor(
        name="alpha-offset",
        semver="1.0.0",
        params=[
            {"name": "offset", "unit": "si:m", "min": -0.01, "max": 0.01, "source_kinds": ["certificate", "dcc"]},
        ],
        template=tmpl,
        applicability=["cmm", "thermal"],
        references=[{"designator": "ISO 1:2022", "clause": "Clause 4"}],
    )


def make_beta_drift() -> dict:
    tmpl = {
        "name": "beta-drift",
        "semver": "1.1.0",
        "inputs": [
            {
                "id": "drift_rate",
                "quantity_kind": "length",
                "unit": "si:m",
                "distribution": {
                    "kind": "normal",
                    "params": {"mu": f64_hex(0.0), "sigma": f64_hex(5e-6)},
                },
            },
            {
                "id": "elapsed_ratio",
                "quantity_kind": "ratio",
                "unit": "si:1",
                "distribution": {
                    "kind": "rect",
                    "params": {"a": f64_hex(0.5), "b": f64_hex(1.5)},
                },
            },
        ],
        "operators": [
            make_operator(
                "drift_op",
                "unc.op.scale",
                "1.1.0",
                {"ratio": "$param:elapsed_ratio"},
                [
                    {"arity": "1", "name": "x", "quantity_kind": "length", "unit": "si:m"},
                    {"arity": "1", "name": "k", "quantity_kind": "ratio", "unit": "si:1"},
                ],
                [{"name": "y", "quantity_kind": "length", "unit": "si:m"}],
            )
        ],
        "outputs": [{"id": "drift_out", "quantity_kind": "length", "unit": "si:m"}],
        "edges": [
            {"from": {"node": "drift_rate", "port": "value"}, "to": {"node": "drift_op", "port": "x"}},
            {"from": {"node": "elapsed_ratio", "port": "value"}, "to": {"node": "drift_op", "port": "k"}},
            {"from": {"node": "drift_op", "port": "y"}, "to": {"node": "drift_out", "port": "value"}},
        ],
        "correlations": [],
    }
    return make_contributor(
        name="beta-drift",
        semver="1.1.0",
        params=[
            {"name": "drift_rate", "unit": "si:m", "min": -0.005, "max": 0.005, "source_kinds": ["qualification"]},
            {"name": "elapsed_ratio", "unit": "si:1", "min": 0.0, "max": 10.0, "source_kinds": ["declared", "type_a_dataset"]},
        ],
        template=tmpl,
        applicability=["drift", "optical"],
        references=[{"designator": "JCGM 100:2008", "clause": "Clause 4.3"}],
    )


def make_files() -> dict[str, str]:
    uc_example = make_contributor(
        name="example-gain",
        semver="1.0.0",
        params=[
            {"name": "gain", "unit": "si:1", "min": 0.5, "max": 2.0, "source_kinds": ["certificate", "dcc"]},
            {
                "name": "length_in",
                "unit": "si:m",
                "min": 0.001,
                "max": 1.0,
                "source_kinds": ["certificate", "dcc", "type_a_dataset"],
            },
        ],
        template=example_template("unc.op.scale", "1.0.0"),
        applicability=["cmm", "example", "length"],
        references=[{"designator": "JCGM 100:2008", "clause": "Clause 4.1"}],
    )

    uc_unknown_op = make_contributor(
        name="example-unknown-op",
        semver="1.0.0",
        params=[
            {"name": "gain", "unit": "si:1", "min": 0.5, "max": 2.0, "source_kinds": ["certificate", "dcc"]},
            {
                "name": "length_in",
                "unit": "si:m",
                "min": 0.001,
                "max": 1.0,
                "source_kinds": ["certificate", "dcc", "type_a_dataset"],
            },
        ],
        template=example_template("unc.op.nonexistent", "9.9.9"),
        applicability=["example"],
        references=[{"designator": "JCGM 100:2008", "clause": "Clause 4.1"}],
    )

    # Independent expansion of `uc_example` with instance_id = "inst1"
    inst = "inst1"
    gain_val = 1.25
    length_val = 0.25
    gain_src = {"author": "DCC Lab Fixture", "digest": sha256_label("fixture gain dcc"), "kind": "dcc"}
    length_src = {"author": "Cal Lab Fixture", "digest": sha256_label("fixture length cert"), "kind": "certificate"}

    expanded_model = canonicalize_model(
        {
            "name": "example-gain",
            "semver": "1.0.0",
            "inputs": [
                {
                    "id": f"{inst}.length_in",
                    "quantity_kind": "length",
                    "unit": "si:m",
                    "distribution": {
                        "kind": "normal",
                        "params": {"mu": f64_hex(length_val), "sigma": f64_hex(1e-5)},
                    },
                    "source": length_src,
                },
                {
                    "id": f"{inst}.gain",
                    "quantity_kind": "ratio",
                    "unit": "si:1",
                    "distribution": {
                        "kind": "normal",
                        "params": {"mu": f64_hex(gain_val), "sigma": f64_hex(1e-4)},
                    },
                    "source": gain_src,
                },
            ],
            "operators": [
                make_operator(
                    f"{inst}.scale_op",
                    "unc.op.scale",
                    "1.0.0",
                    {"factor": f64_hex(gain_val)},
                    [
                        {"arity": "1", "name": "x", "quantity_kind": "length", "unit": "si:m"},
                        {"arity": "1", "name": "k", "quantity_kind": "ratio", "unit": "si:1"},
                    ],
                    [{"name": "y", "quantity_kind": "length", "unit": "si:m"}],
                )
            ],
            "outputs": [{"id": f"{inst}.scaled_length", "quantity_kind": "length", "unit": "si:m"}],
            "edges": [
                {"from": {"node": f"{inst}.length_in", "port": "value"}, "to": {"node": f"{inst}.scale_op", "port": "x"}},
                {"from": {"node": f"{inst}.gain", "port": "value"}, "to": {"node": f"{inst}.scale_op", "port": "k"}},
                {
                    "from": {"node": f"{inst}.scale_op", "port": "y"},
                    "to": {"node": f"{inst}.scaled_length", "port": "value"},
                },
            ],
            "correlations": [],
        }
    )
    expected_expand_sha = sha256_bytes(canon(expanded_model))

    alpha = make_alpha_offset()
    beta = make_beta_drift()
    lib_entries = sorted([uc_example, alpha, beta], key=lambda c: c["id"])

    def summary(c: dict) -> dict:
        return {
            "applicability": c["applicability"],
            "digest": c["digest"],
            "id": c["id"],
            "params": [p["name"] for p in c["params"]],
            "semver": c["semver"],
        }

    expected_list = {
        "all": {"contributors": [summary(c) for c in lib_entries]},
        "cmm": {"contributors": [summary(c) for c in lib_entries if "cmm" in c["applicability"]]},
    }

    readme = (
        "# UNC.MODEL.CONTRIB.CONTRACT test fixtures\n"
        "\n"
        "License: CC-BY-4.0.\n"
        "\n"
        "Generated by `make_fixtures.py` (`python3 fixtures/unc/model/contrib/make_fixtures.py --check`).\n"
        "\n"
        "- `uc_example.json`: invented two-parameter contributor (`uc:example-gain@1.0.0`) for AC-1 and AC-2.\n"
        "- `uc_unknown_op.json`: contributor referencing an operator absent from the kernel manifest (AC-2 `unknown_operator`).\n"
        "- `expected_expand.sha256`: independent SHA-256 of the expanded `uncertainty-model/v1` sub-graph for `inst1` (AC-1).\n"
        "- `library/`: three-contributor library stub (`uc:alpha-offset@1.0.0`, `uc:beta-drift@1.1.0`, `uc:example-gain@1.0.0`) for AC-3.\n"
        "- `expected_list.json`: independent canonical summary expectations for `unc.list_contributor@1`.\n"
    )

    pretty = lambda obj: json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    return {
        "README.md": readme,
        "uc_example.json": pretty(uc_example),
        "uc_unknown_op.json": pretty(uc_unknown_op),
        "expected_expand.sha256": expected_expand_sha + "\n",
        "expected_list.json": pretty(expected_list),
        "library/uc_alpha_offset.json": pretty(alpha),
        "library/uc_beta_drift.json": pretty(beta),
        "library/uc_example.json": pretty(uc_example),
    }


def main() -> int:
    check_mode = "--check" in sys.argv[1:]
    files = make_files()

    if check_mode:
        errors: list[str] = []
        for rel, want in sorted(files.items()):
            path = HERE / rel
            if not path.is_file():
                errors.append(f"missing fixture: {rel}")
                continue
            got = path.read_text(encoding="utf-8")
            if got != want:
                errors.append(f"fixture out of date: {rel}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    for rel, content in sorted(files.items()):
        path = HERE / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
