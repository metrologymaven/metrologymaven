#!/usr/bin/env python3
"""Independent reference for HAL.IDL.BASE.CFG fixtures (CC-BY-4.0 project data).

Writes, next to this script:
  linescan_cfg.schema.json      the setting schema of a fictional line-scan sensor, with two
                                vendor extensions declared under `acme.` (AC-1..AC-4)
  linescan_cfg.json             a complete configuration for that schema (AC-1)
  linescan_cfg.canonical.json   RFC 8785 canonical bytes of its digested subset
  linescan_cfg.sha256           sha256 of those bytes, as "sha256:<hex>" (AC-1)
  digest_cases.json             one-setting variations of linescan_cfg.json with their expected
                                digests (AC-2, AC-3)
  invalid_cases.json            configurations that must fail validation, with the expected
                                code, setting and reason (AC-4 and further rules)
  schema_invalid_cases.json     schemas that must be refused, with the expected setting and
                                reason (code is always CONFIG_INVALID)

Digests come from this script's own RFC 8785 writer and hashlib, never from the Rust crate.
The digested subset is chosen here from the schema dict: settings marked
acquisition_relevant plus every vendor extension (a name with a dot), as
REQ-HAL.IDL.BASE.CFG-002 states. Expected errors are hand-labelled from the leaf's rules
and the wire rules documented in crates/fid-hal-idl/src/base/config.rs:
  configuration/0 = {"schema": "configuration/0", "values": {name: value}};
  bool as JSON bool, int as canonical decimal string, quantity as
  {"unit": sym, "value": "0x" + 16 hex digits}, enum and text as strings;
  every declared setting must be present; undeclared names are refused;
  the first failing setting in sorted name order is reported.

Run:    python fixtures/hal/idl/base/cfg/make_fixtures.py
Check:  python fixtures/hal/idl/base/cfg/make_fixtures.py --check   (CI; exits 1 if any file differs)
"""

import copy
import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUTPUTS = {}


# ------------------------------------------------------------------ RFC 8785 writer

def jcs(v):
    """RFC 8785 for the value shapes used here (no JSON numbers appear)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, str):
        # With ensure_ascii=False Python escapes exactly '"', '\\' and U+0000..U+001F
        # (short forms for \b \t \n \f \r, lowercase \u00xx otherwise), as RFC 8785 does.
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError("unexpected value type: %r" % (v,))


def canonical(v):
    return jcs(v).encode("utf-8")


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def f64_bits(x):
    return "0x" + struct.pack(">d", x).hex()


def qty(x, unit):
    return {"unit": unit, "value": f64_bits(x)}


# ------------------------------------------------------------------ schema and configuration

def setting(name, ty, *, unit=None, values=None, rng=None, acq, live):
    return {
        "acquisition_relevant": acq,
        "live_changeable": live,
        "name": name,
        "range": rng,
        "type": ty,
        "unit": unit,
        "values": values,
    }


def schema():
    return {
        "schema": "config-schema/0",
        "settings": [
            setting("display_name", "text", acq=False, live=True),
            setting("exposure", "quantity", unit="s", rng={"max": f64_bits(0.01), "min": f64_bits(1e-6)},
                    acq=True, live=False),
            setting("line_period", "quantity", unit="s", rng={"max": f64_bits(0.1), "min": f64_bits(5e-5)},
                    acq=True, live=False),
            setting("averaging", "int", rng={"max": "64", "min": "1"}, acq=True, live=False),
            setting("trigger_mode", "enum", values=["encoder", "free_run", "software"], acq=True, live=False),
            setting("roi_start", "quantity", unit="m", rng={"max": f64_bits(0.05), "min": f64_bits(0.0)},
                    acq=True, live=False),
            setting("status_led", "bool", acq=False, live=True),
            setting("acme.gain", "int", rng={"max": "16", "min": "0"}, acq=True, live=False),
            setting("acme.note", "text", acq=True, live=True),
        ],
    }


def config():
    return {
        "schema": "configuration/0",
        "values": {
            "acme.gain": "4",
            "acme.note": "Kalibrierlauf Ω µm",
            "averaging": "8",
            "display_name": "Linie 3 – Einlauf",
            "exposure": qty(2.5e-4, "s"),
            "line_period": qty(1e-3, "s"),
            "roi_start": qty(0.0125, "m"),
            "status_led": True,
            "trigger_mode": "encoder",
        },
    }


def digested_subset(sch, cfg):
    """REQ-002: acquisition-relevant settings plus every extension (`vendor.name`)."""
    keep = {s["name"] for s in sch["settings"] if s["acquisition_relevant"] or "." in s["name"]}
    return {k: v for k, v in cfg["values"].items() if k in keep}


def cfg_digest(sch, cfg):
    return digest(canonical(digested_subset(sch, cfg)))


def with_value(name, value):
    c = config()
    c["values"][name] = value
    return c


def without_value(name):
    c = config()
    del c["values"][name]
    return c


# ------------------------------------------------------------------ cases

def digest_cases():
    base = cfg_digest(schema(), config())
    variations = [
        # AC-2: not acquisition-relevant, digest unchanged.
        ("display_name_changed", "display_name", "Linie 4"),
        ("status_led_changed", "status_led", False),
        # AC-2: acquisition-relevant, digest changes.
        ("exposure_changed", "exposure", qty(5e-4, "s")),
        ("roi_start_negative_zero", "roi_start", qty(-0.0, "m")),
        ("averaging_changed", "averaging", "16"),
        ("trigger_mode_changed", "trigger_mode", "software"),
        # AC-3: declared extensions always enter the digest.
        ("acme_gain_changed", "acme.gain", "5"),
        ("acme_note_changed", "acme.note", "Kalibrierlauf Ω µm."),
    ]
    out = [{"name": "base", "config": config(), "sha256": base, "same_as_base": True}]
    for name, key, value in variations:
        c = with_value(key, value)
        d = cfg_digest(schema(), c)
        out.append({"name": name, "config": c, "sha256": d, "same_as_base": d == base})
    # Hand-labelled expectations: the oracle's filter must agree with the leaf's rules.
    expect_same = {"base", "display_name_changed", "status_led_changed"}
    for case in out:
        assert case["same_as_base"] == (case["name"] in expect_same), case["name"]
    return out


def invalid_cases():
    assert 0.05000000000000001 != 0.05 and 0.05000000000000001 > 0.05
    inv, rng = "CONFIG_INVALID", "OUT_OF_RANGE"
    c_two = with_value("exposure", qty(0.5, "s"))
    c_two["values"]["acme.secret"] = "1"
    c_schema = config()
    c_schema["schema"] = "configuration/1"
    c_extra = config()
    c_extra["vendor"] = "acme"
    c_notobj = config()
    c_notobj["values"] = []
    cases = [
        # AC-4
        ("undeclared_extension", with_value("acme.secret", "1"), inv, "acme.secret", "undeclared"),
        ("exposure_above_range", with_value("exposure", qty(0.5, "s")), rng, "exposure", "range"),
        # Further rules of REQ-004 and the wire rules.
        ("undeclared_base_setting", with_value("gain", "1"), inv, "gain", "undeclared"),
        ("undeclared_other_vendor", with_value("zeta.gain", "1"), inv, "zeta.gain", "undeclared"),
        ("first_in_name_order", c_two, inv, "acme.secret", "undeclared"),
        ("exposure_below_range", with_value("exposure", qty(1e-7, "s")), rng, "exposure", "range"),
        ("exposure_wrong_unit", with_value("exposure", qty(2.5e-4, "m")), inv, "exposure", "wrong_unit"),
        ("exposure_unit_ms", with_value("exposure", {"unit": "ms", "value": f64_bits(0.25)}), inv,
         "exposure", "wrong_unit"),
        ("exposure_nan", with_value("exposure", {"unit": "s", "value": "0x7ff8000000000000"}), inv,
         "exposure", "not_finite"),
        ("exposure_bad_bits", with_value("exposure", {"unit": "s", "value": "0x3F30"}), inv, "exposure",
         "bad_value"),
        ("exposure_extra_field", with_value("exposure", {"unit": "s", "value": f64_bits(2.5e-4), "x": "1"}),
         inv, "exposure", "bad_value"),
        ("exposure_plain_string", with_value("exposure", "2.5e-4"), inv, "exposure", "wrong_type"),
        ("roi_at_max_plus", with_value("roi_start", qty(0.05000000000000001, "m")), rng, "roi_start",
         "range"),
        ("averaging_zero", with_value("averaging", "0"), rng, "averaging", "range"),
        ("averaging_65", with_value("averaging", "65"), rng, "averaging", "range"),
        ("averaging_leading_zero", with_value("averaging", "08"), inv, "averaging", "bad_value"),
        ("averaging_bool", with_value("averaging", True), inv, "averaging", "wrong_type"),
        ("acme_gain_17", with_value("acme.gain", "17"), rng, "acme.gain", "range"),
        ("trigger_unknown", with_value("trigger_mode", "external"), inv, "trigger_mode", "enum"),
        ("status_led_string", with_value("status_led", "true"), inv, "status_led", "wrong_type"),
        ("display_name_too_long", with_value("display_name", "x" * 257), inv, "display_name", "too_long"),
        ("display_name_control", with_value("display_name", "a\u0007b"), inv, "display_name", "bad_value"),
        ("missing_roi_start", without_value("roi_start"), inv, "roi_start", "missing"),
        ("missing_extension", without_value("acme.gain"), inv, "acme.gain", "missing"),
        ("schema_v1", c_schema, inv, "", "schema"),
        ("unknown_top_field", c_extra, inv, "", "schema"),
        ("values_not_object", c_notobj, inv, "", "schema"),
    ]
    return [
        {"name": n, "config": c, "code": code, "setting": s, "reason": r}
        for (n, c, code, s, r) in cases
    ]


def with_setting(index, key, value):
    s = schema()
    s["settings"][index][key] = value
    return s


def schema_invalid_cases():
    dup = schema()
    dup["settings"].append(copy.deepcopy(dup["settings"][1]))
    unknown = schema()
    unknown["settings"][0]["default"] = "x"
    missing = schema()
    del missing["settings"][3]["range"]
    v1 = schema()
    v1["schema"] = "config-schema/1"
    cases = [
        ("duplicate_name", dup, "exposure", "duplicate"),
        ("upper_case_name", with_setting(1, "name", "Exposure"), "Exposure", "bad_name"),
        ("empty_vendor", with_setting(1, "name", ".gain"), ".gain", "bad_name"),
        ("two_dots", with_setting(7, "name", "acme.x.gain"), "acme.x.gain", "bad_name"),
        ("quantity_without_unit", with_setting(1, "unit", None), "exposure", "unit"),
        ("quantity_unit_mm", with_setting(1, "unit", "mm"), "exposure", "unit"),
        ("unit_on_bool", with_setting(6, "unit", "1"), "status_led", "unit"),
        ("range_on_enum", with_setting(4, "range", {"max": "1", "min": "0"}), "trigger_mode", "range"),
        ("range_reversed", with_setting(3, "range", {"max": "1", "min": "2"}), "averaging", "range"),
        ("range_nan", with_setting(1, "range", {"max": "0x7ff8000000000000", "min": f64_bits(0.0)}),
         "exposure", "range"),
        ("enum_empty", with_setting(4, "values", []), "trigger_mode", "values"),
        ("enum_duplicate", with_setting(4, "values", ["encoder", "encoder"]), "trigger_mode", "values"),
        ("values_on_int", with_setting(3, "values", ["1"]), "averaging", "values"),
        ("unknown_type", with_setting(3, "type", "float"), "averaging", "type"),
        ("extension_not_relevant", with_setting(7, "acquisition_relevant", False), "acme.gain",
         "extension_flag"),
        ("unknown_setting_field", unknown, "display_name", "schema"),
        ("missing_setting_field", missing, "averaging", "schema"),
        ("schema_v1", v1, "", "schema"),
    ]
    return [{"name": n, "schema": s, "setting": st, "reason": r} for (n, s, st, r) in cases]


# ------------------------------------------------------------------ output

def write_json(name, value):
    write_bytes(name, (json.dumps(value, indent=2, ensure_ascii=True) + "\n").encode("ascii"))


def write_cases(name, cases):
    # One case per line keeps the file reviewable.
    lines = ",\n".join("  " + json.dumps(case, ensure_ascii=True, sort_keys=True) for case in cases)
    write_bytes(name, ("[\n" + lines + "\n]\n").encode("ascii"))


def write_bytes(name, data):
    OUTPUTS[name] = data


def flush(check):
    """Writes every output, or with check=True compares them with the committed files."""
    stale = []
    for name, data in sorted(OUTPUTS.items()):
        path = os.path.join(HERE, name)
        if check:
            try:
                with open(path, "rb") as fh:
                    same = fh.read() == data
            except FileNotFoundError:
                same = False
            if not same:
                stale.append(name)
        else:
            with open(path, "wb") as fh:
                fh.write(data)
    if stale:
        print("stale fixtures (re-run make_fixtures.py): " + ", ".join(stale), file=sys.stderr)
        return 1
    return 0


def main():
    check = "--check" in sys.argv[1:]
    sch, cfg = schema(), config()
    subset = canonical(digested_subset(sch, cfg))
    d = digest(subset)
    write_json("linescan_cfg.schema.json", sch)
    write_json("linescan_cfg.json", cfg)
    write_bytes("linescan_cfg.canonical.json", subset)
    write_bytes("linescan_cfg.sha256", (d + "\n").encode("ascii"))
    write_cases("digest_cases.json", digest_cases())
    write_cases("invalid_cases.json", invalid_cases())
    write_cases("schema_invalid_cases.json", schema_invalid_cases())
    status = flush(check)
    if status == 0:
        print("linescan_cfg digest:", d, "(checked)" if check else "(written)")
    return status


if __name__ == "__main__":
    sys.exit(main())
