#!/usr/bin/env python3
"""Independent reference for GDT.CHAR.RECORD fixtures (stdlib only).

Writes the synthetic builder inputs of AC-1 (`flatness_inputs.json`) and, for each case
below, the RFC 8785 canonical JSON the ResultRecord must have (`expected/<case>.json`),
following the record wire form pinned in the leaf's module documentation
(`fid.gdt-result/1`):

- every CONSTITUTION section 5 field plus `dmt_row`, `operator_chain_digest`, `dmt_digest`;
- `value` and the uncertainty numbers as IEEE-754 bit patterns `"0x"` + 16 hex digits;
- `uncertainty` copied from the UNC statement fixture (`../../unc/stmt/valid/`), with the
  evidence facts derived here from the statement's own rules: `claimed` when the method is
  `DECLARED` or `SPEC_DERIVED`; `declared_input` when any source kind is `declared`;
  `uncharacterized_input` when any source kind is not `dcc`, `qualification` or `type_a`;
  `statement` is the SHA-256 of the statement's canonical bytes;
- `decision` from the bound rule stub: `source` `unc` with the stub's rule, outcome and the
  statement's basis facts, or `gdt_not_decidable` with reason `no_rule_bound` and no rule.

Cases:
- `flatness_conforms` (AC-1): UEC-2 LPU statement, stub `dr:simple-acceptance@1` -> CONFORMS;
- `no_rule_bound` (AC-3): same inputs, no rule bound;
- `claimed_conforms`: a DECLARED (claimed) UEC-1 statement under the same stub, so the
  record must say `claimed: true` in both `uncertainty` and `decision.basis`.

The JCS serializer here is this script's own, not the Rust code under test. With `--check`
it rebuilds every file in memory and fails if a committed file differs byte for byte.

Everything is synthetic placeholder data with no standards text; CC0.
"""

import hashlib
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STMT = HERE.parent.parent / "unc" / "stmt" / "valid"
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}
CHARACTERIZED = {"dcc", "qualification", "type_a"}


def jcs_string(s):
    out = ['"']
    for c in s:
        if c in SHORT:
            out.append(SHORT[c])
        elif ord(c) < 0x20:
            out.append("\\u%04x" % ord(c))
        else:
            out.append(c)
    out.append('"')
    return "".join(out)


def jcs(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def bits(x):
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


def stmt_bits(h):
    # Statement fixtures write numbers as 16 hex digits without a prefix.
    assert len(h) == 16
    return "0x" + h


INPUTS = {
    "run": "run-2026-11-19-001",
    "characteristic": "flat-1",
    "measurand": "flatness of datum face A",
    "evaluation_basis": "POINTS",
    "value": bits(0.0000123),
    "unit": "si:m",
    "uncertainty_statement": "uec2_lpu.json",
    "spec": {
        "dialect": "ASME",
        "standards": ["std:asme:y14.5:2018"],
        "defaults_profile": "dp:asme-y14.5-2018@1.0.0#sha256:"
        + "287a04d6c384768b1e44c85b3c4ae851912d700b723dcd28b72e395065ec84dc",
        "inherited_from": "explicit",
    },
    "algorithm": "gdt.form.flatness.minzone@1.0.0",
    "kernel_version": "0.1.0",
    "input_digests": sorted([sha("synthetic points of flat-1"), sha("synthetic feature fit of flat-1")]),
    "journal_entry": sha("synthetic journal entry 1"),
    "dmt_row": "dmt:asme/flatness/none@1",
    "operator_chain_digest": sha("synthetic operator chain"),
    "dmt_digest": sha("synthetic dmt table"),
    "rule_stub": {"rule": "dr:simple-acceptance@1", "outcome": "CONFORMS"},
}


def uncertainty(stmt_bytes):
    s = json.loads(stmt_bytes)
    method = s.get("method")
    kinds = [src["kind"] for src in s.get("sources", [])]
    e = s.get("expanded")
    i = s.get("interval")
    u = {
        "class": s["class"],
        "uec0_reason": s.get("uec0_reason"),
        "method": method,
        "statement": "sha256:" + hashlib.sha256(stmt_bytes).hexdigest(),
        "unit": s.get("unit"),
        "u_std": stmt_bits(s["u_std"]) if "u_std" in s else None,
        "expanded": {"U": stmt_bits(e["U"]), "k": stmt_bits(e["k"])} if e else None,
        "interval": (
            {"low": stmt_bits(i["low"]), "high": stmt_bits(i["high"]), "p": stmt_bits(i["p"]), "kind": i["kind"]}
            if i
            else None
        ),
        "claimed": method in ("DECLARED", "SPEC_DERIVED"),
        "declared_input": any(k == "declared" for k in kinds),
        "uncharacterized_input": any(k not in CHARACTERIZED for k in kinds),
    }
    return u


def record(stmt_name, rule_bound):
    stmt_bytes = (STMT / stmt_name).read_bytes()
    u = uncertainty(stmt_bytes)
    if rule_bound:
        decision = {
            "source": "unc",
            "rule": INPUTS["rule_stub"]["rule"],
            "outcome": INPUTS["rule_stub"]["outcome"],
            "reason": None,
            "basis": {
                "class": u["class"],
                "method": u["method"],
                "claimed": u["claimed"],
                "declared_input": u["declared_input"],
                "uncharacterized_input": u["uncharacterized_input"],
                "statement": u["statement"],
            },
        }
    else:
        decision = {"source": "gdt_not_decidable", "rule": None, "outcome": "NOT_DECIDABLE",
                    "reason": "no_rule_bound", "basis": None}
    r = {"schema": "fid.gdt-result/1"}
    for k in ("run", "characteristic", "measurand", "evaluation_basis", "spec", "algorithm", "kernel_version",
              "input_digests", "journal_entry", "dmt_row", "operator_chain_digest", "dmt_digest"):
        r[k] = INPUTS[k]
    r["value"] = {"value": INPUTS["value"], "unit": INPUTS["unit"]}
    r["uncertainty"] = u
    r["decision"] = decision
    return jcs(r)


def files():
    out = {"flatness_inputs.json": json.dumps(INPUTS, indent=2) + "\n"}
    out["expected/flatness_conforms.json"] = record("uec2_lpu.json", True)
    out["expected/no_rule_bound.json"] = record("uec2_lpu.json", False)
    out["expected/claimed_conforms.json"] = record("uec1_declared.json", True)
    return out


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    for name, text in files().items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if bad:
        print("stale: " + ", ".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
