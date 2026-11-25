#!/usr/bin/env python3
"""Independent reference for UNC.INST.SCHEMA fixtures (stdlib only; CC-BY-4.0 data).

Written from spec/unc/inst/schema/NODE.md and the module documentation of crate
`fid-unc-inst`, never from the Rust code. Every expected value is a SHA-256 from hashlib, a
byte length, an IEEE-754 bit pattern from `struct`, or text labelled by hand from the rules
below.

INVENTED DATA (D-038, D-045). Every model here is a made-up example. The parameter values are
round numbers chosen for tests; they describe no real instrument and were never measured. The
source citation of every model says so. The device `sim-tracker-1` under `standin/` is a
STAND-IN written for this leaf: the HAL simulator (HAL.SIM, HAL.SIM.PORT) that should supply
it is not built yet, so AC-3 stays open until a test binds against the simulator's own
descriptor.

Rules the expected values follow:
  - `instrument-model/v1` canonical form: an object with exactly the members
    device_class, id, kind, params, range, schema, source; params sorted by name, range axes
    sorted by axis name; every number is the string "0x" + 16 lowercase hex digits of the
    IEEE-754 double in the SI unit (m, rad, K, s or 1), negative zero written as +0;
  - a decimal value with a prefixed unit is converted by one correctly rounded step: the
    exact decimal value times 10^shift, rounded once to the nearest double (here
    fractions.Fraction, whose float() is correctly rounded);
  - RFC 8785 canonical JSON (members sorted by UTF-16 code units, no whitespace); all text
    here is ASCII;
  - the model digest is SHA-256 of the canonical bytes; a model up to 8 MiB is one
    content-addressed blob whose address is that digest;
  - journal events and results are the member sets documented in fid-unc-inst `event` and
    `command`, labelled by hand below;
  - `check_range`: closed bounds; an observed axis below min is `out_of_model_range` side
    `lower` with bound min, above max side `upper` with bound max; out-of-range on any axis
    is reported before an axis that could not be checked; axes are visited in canonical
    (sorted) order; an axis the context does not hold is `not_checked` / `not_observed`, one
    in another SI unit `not_checked` / `unit_mismatch`.

Usage: make_fixtures.py [--check]   (--check compares with the files on disk)
"""

import base64
import copy
import hashlib
import json
import os
import struct
import sys
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))

CITATION = (
    "Invented example model for tests: the parameter values are made up and describe no real "
    "instrument (project fixture, D-038, CC-BY-4.0)"
)

# unit symbol -> (SI unit, power of ten)
UNITS = {
    "1": ("1", 0),
    "K": ("K", 0),
    "km": ("m", 3),
    "m": ("m", 0),
    "mK": ("K", -3),
    "mm": ("m", -3),
    "mrad": ("rad", -3),
    "ms": ("s", -3),
    "nm": ("m", -9),
    "ppm": ("1", -6),
    "rad": ("rad", 0),
    "s": ("s", 0),
    "um": ("m", -6),
    "um/m": ("1", -6),
    "urad": ("rad", -6),
    "us": ("s", -6),
}

DIST_PARAMS = {
    "normal": ["mu", "sigma"],
    "rect": ["a", "b"],
    "tri": ["a", "b", "c"],
    "arcsine": ["a", "b"],
    "scaled_t": ["mu", "s", "nu"],
    "trap": ["a", "b", "beta"],
}


# ------------------------------------------------------------------ canonical JSON

def jcs(v):
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        return json.dumps(v, ensure_ascii=False)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError("unexpected value type: %r" % (v,))


def canonical(v):
    return jcs(v).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def bits(x):
    if x == 0.0:
        x = 0.0
    return "0x" + struct.pack(">d", x).hex()


def si(value, unit):
    """The SI double of a decimal string in `unit` (one rounding) and the SI unit."""
    target, shift = UNITS[unit]
    exact = Fraction(value) * (Fraction(10) ** shift)
    return float(exact), target


# ------------------------------------------------------------------ the reference reader

def canonical_model(doc):
    """The canonical value of a valid input document (fixtures here are valid by
    construction; the invalid ones are labelled by hand in INVALID)."""
    params = []
    for p in doc["params"]:
        x, u = si(p["value"], p["unit"])
        out = {"name": p["name"], "unit": u, "value": bits(x)}
        if "dist" in p:
            d = p["dist"]
            dp = {}
            for n in DIST_PARAMS[d["kind"]]:
                if n in ("nu", "beta"):
                    v, _ = si(d["params"][n], "1")
                else:
                    v, _ = si(d["params"][n], p["unit"])
                dp[n] = bits(v)
            out["dist"] = {"kind": d["kind"], "params": dp}
        params.append(out)
    params.sort(key=lambda p: p["name"])
    axes = []
    for a in doc["range"]:
        lo, u = si(a["min"], a["unit"])
        hi, _ = si(a["max"], a["unit"])
        assert lo < hi
        axes.append({"axis": a["axis"], "max": bits(hi), "min": bits(lo), "unit": u})
    axes.sort(key=lambda a: a["axis"])
    return {
        "device_class": doc["device_class"],
        "id": doc["id"],
        "kind": doc["kind"],
        "params": params,
        "range": axes,
        "schema": "instrument-model/v1",
        "source": doc["source"],
    }


# ------------------------------------------------------------------ example models

def normal(mu, sigma):
    return {"kind": "normal", "params": {"mu": mu, "sigma": sigma}}


def tracker():
    return {
        "schema": "instrument-model/v1",
        "id": "im:tracker-example@1.0.0",
        "kind": "observation",
        "device_class": "tracker",
        "params": [
            {"name": "range_const_sigma", "unit": "um", "value": "15", "dist": normal("0", "15")},
            {"name": "range_prop_sigma", "unit": "um/m", "value": "6", "dist": normal("0", "6")},
            {"name": "angle_sigma", "unit": "urad", "value": "5", "dist": normal("0", "5")},
            {"name": "probing_noise", "unit": "um", "value": "10",
             "dist": {"kind": "rect", "params": {"a": "-10", "b": "10"}}},
        ],
        "range": [
            {"axis": "range", "min": "0.5", "max": "40", "unit": "m"},
            {"axis": "temperature", "min": "283.15", "max": "313.15", "unit": "K"},
        ],
        "source": CITATION,
    }


def tracker_respelled():
    """The same model in other units and another member order: the same canonical bytes."""
    return {
        "source": CITATION,
        "range": [
            {"unit": "mK", "axis": "temperature", "min": "283150", "max": "313150"},
            {"unit": "mm", "axis": "range", "min": "500", "max": "40000"},
        ],
        "params": [
            {"name": "probing_noise", "unit": "mm", "value": "0.01",
             "dist": {"kind": "rect", "params": {"a": "-0.01", "b": "1e-2"}}},
            {"name": "angle_sigma", "unit": "mrad", "value": "0.005", "dist": normal("-0", "5e-3")},
            {"name": "range_prop_sigma", "unit": "ppm", "value": "6.0", "dist": normal("0", "6")},
            {"name": "range_const_sigma", "unit": "mm", "value": "0.015", "dist": normal("0", "0.015")},
        ],
        "device_class": "tracker",
        "kind": "observation",
        "id": "im:tracker-example@1.0.0",
        "schema": "instrument-model/v1",
    }


def tracker_conflict():
    """Same id, different content (range axis to 50 m): AC-5."""
    d = tracker()
    d["range"][0]["max"] = "50"
    return d


def arm():
    return {
        "schema": "instrument-model/v1",
        "id": "im:arm-example@1.0.0",
        "kind": "spec_derived",
        "device_class": "arm",
        "params": [
            {"name": "length_error_const", "unit": "um", "value": "20"},
            {"name": "probing_size", "unit": "um", "value": "15"},
            {"name": "probing_form", "unit": "um", "value": "20",
             "dist": {"kind": "tri", "params": {"a": "-20", "b": "20", "c": "0"}}},
        ],
        "range": [
            {"axis": "reach", "min": "0", "max": "1.2", "unit": "m"},
            {"axis": "temperature", "min": "288.15", "max": "303.15", "unit": "K"},
        ],
        "source": CITATION,
    }


def scanner():
    return {
        "schema": "instrument-model/v1",
        "id": "im:scanner-example@1.0.0",
        "kind": "observation",
        "device_class": "scanner",
        "params": [
            {"name": "point_noise", "unit": "um", "value": "25",
             "dist": {"kind": "scaled_t", "params": {"mu": "0", "s": "25", "nu": "10"}}},
            {"name": "incidence_gain", "unit": "1", "value": "0.5",
             "dist": {"kind": "trap", "params": {"a": "0", "b": "1", "beta": "0.5"}}},
        ],
        "range": [
            {"axis": "standoff", "min": "100", "max": "300", "unit": "mm"},
            {"axis": "incidence", "min": "0", "max": "1", "unit": "rad"},
        ],
        "source": CITATION,
    }


def cmm():
    return {
        "schema": "instrument-model/v1",
        "id": "im:cmm-example@1.0.0",
        "kind": "spec_derived",
        "device_class": "cmm",
        "params": [
            {"name": "length_error_const", "unit": "um", "value": "1.5",
             "dist": {"kind": "arcsine", "params": {"a": "-1.5", "b": "1.5"}}},
            {"name": "length_error_prop", "unit": "ppm", "value": "2"},
            {"name": "probing_form", "unit": "um", "value": "2"},
            {"name": "dwell", "unit": "ms", "value": "50"},
        ],
        "range": [
            {"axis": "length", "min": "0", "max": "1", "unit": "m"},
            {"axis": "temperature", "min": "291.15", "max": "295.15", "unit": "K"},
        ],
        "source": CITATION,
    }


EXAMPLES = {
    "tracker_example.json": tracker,
    "tracker_example_respelled.json": tracker_respelled,
    "tracker_example_conflict.json": tracker_conflict,
    "arm_example.json": arm,
    "scanner_example.json": scanner,
    "cmm_example.json": cmm,
}


# ------------------------------------------------------------------ invalid variants

def without(d, path):
    d = copy.deepcopy(d)
    tgt = d
    for k in path[:-1]:
        tgt = tgt[k]
    del tgt[path[-1]]
    return d


def with_value(d, path, value):
    d = copy.deepcopy(d)
    tgt = d
    for k in path[:-1]:
        tgt = tgt[k]
    tgt[path[-1]] = value
    return d


def invalid():
    t = tracker()
    dup = copy.deepcopy(t)
    dup["params"][1]["name"] = "range_const_sigma"
    dup_axis = copy.deepcopy(t)
    dup_axis["range"][1]["axis"] = "range"
    extra = copy.deepcopy(t)
    extra["notes"] = "x"
    kelvin_dist = copy.deepcopy(t)
    kelvin_dist["params"].append({"name": "warmup", "unit": "K", "value": "1", "dist": normal("0", "1")})
    # file -> (document, expected error code, expected field)
    return {
        "no_unit.json": (without(t, ["params", 0, "unit"]), "missing_unit", "range_const_sigma"),
        "empty_unit.json": (with_value(t, ["params", 2, "unit"], ""), "missing_unit", "angle_sigma"),
        "inverted_range.json": (
            with_value(with_value(t, ["range", 0, "min"], "40"), ["range", 0, "max"], "0.5"),
            "invalid_range", "range"),
        "empty_range_axis.json": (with_value(t, ["range", 1, "max"], "283.15"), "invalid_range", "temperature"),
        "no_source.json": (without(t, ["source"]), "missing_source", "source"),
        "empty_source.json": (with_value(t, ["source"], ""), "missing_source", "source"),
        "control_in_source.json": (with_value(t, ["source"], "line‮override"), "schema_invalid", "/source"),
        "no_range.json": (with_value(t, ["range"], []), "schema_invalid", "/range"),
        "bad_unit.json": (with_value(t, ["params", 0, "unit"], "furlong"), "schema_invalid", "/params/0/unit"),
        "bad_axis_unit.json": (with_value(t, ["range", 0, "unit"], "deg"), "schema_invalid", "/range/0/unit"),
        "bad_value.json": (with_value(t, ["params", 0, "value"], "1,5"), "schema_invalid", "/params/0/value"),
        "hex_with_prefixed_unit.json": (
            with_value(t, ["params", 0, "value"], "0x3ef4f8b588e368f1"), "schema_invalid", "/params/0/value"),
        "overflow_value.json": (with_value(t, ["params", 3, "value"], "1e400"), "schema_invalid", "/params/3/value"),
        "bad_dist.json": (with_value(t, ["params", 0, "dist", "params", "sigma"], "-1"), "schema_invalid",
                          "/params/0/dist/params/sigma"),
        "dist_on_kelvin.json": (kelvin_dist, "schema_invalid", "/params/4/dist"),
        "dup_param.json": (dup, "schema_invalid", "/params/1/name"),
        "dup_axis.json": (dup_axis, "schema_invalid", "/range/1/axis"),
        "bad_id.json": (with_value(t, ["id"], "tracker-example@1.0.0"), "schema_invalid", "/id"),
        "bad_kind.json": (with_value(t, ["kind"], "measured"), "schema_invalid", "/kind"),
        "unknown_field.json": (extra, "schema_invalid", "/notes"),
        "wrong_schema.json": (with_value(t, ["schema"], "instrument-model/v2"), "schema_invalid", "/schema"),
    }


# ------------------------------------------------------------------ stand-in device

def standin_profile(model_digest):
    lo, _ = si("0.5", "m")
    hi, _ = si("40", "m")
    tlo, _ = si("283.15", "K")
    thi, _ = si("313.15", "K")
    return {
        "schema": "instrument-profile/v1",
        "profile_id": "sim-tracker-standin@1.0.0",
        "device_class": ["PortablePose"],
        "make": "Example Tracker Works (invented stand-in)",
        "model": "StandIn Tracker 1",
        "mpe_terms": [],
        "frame_chain": [
            {"name": "reflector", "kind": "sensor"},
            {"name": "tracker-head", "kind": "device"},
        ],
        "validity": {
            "range": {"lower": {"unit": "m", "value": bits(lo)}, "upper": {"unit": "m", "value": bits(hi)}},
            "temperature": {"lower": {"unit": "K", "value": bits(tlo)}, "upper": {"unit": "K", "value": bits(thi)}},
        },
        "source": {
            "kind": "spec-sheet",
            "digest": sha(b"stand-in descriptor for sim-tracker-1, not from HAL.SIM"),
            "citation": "Stand-in descriptor written for UNC.INST.SCHEMA tests until HAL.SIM ships; "
                        "invented values, not a real product (CC-BY-4.0)",
        },
        "um_model": {"id": "im:tracker-example@1.0.0", "digest": model_digest},
        "uec_max": 1,
        "signature": {
            "keyid": "key:fixture-profile-signer-2026",
            "sig": base64.b64encode(b"fixture signature bytes, not a real signature").decode("ascii"),
        },
    }


def text(s):
    return {"bytes": s, "truncated": False}


def standin_instance():
    return {
        "driver_id": "standin-tracker-driver",
        "make": text("Example Tracker Works (invented stand-in)"),
        "model": text("StandIn Tracker 1"),
        "serial": text("EXAMPLE-0001"),
    }


def profile_digest(profile):
    p = copy.deepcopy(profile)
    del p["signature"]
    return sha(canonical(p))


# ------------------------------------------------------------------ range cases

def check_range(model, context):
    """model: canonical value; context: list of (axis, decimal, unit)."""
    ctx = {}
    for axis, value, unit in context:
        x, u = si(value, unit)
        ctx[axis] = (x, u)
    pending = None
    for a in model["range"]:
        lo = struct.unpack(">d", bytes.fromhex(a["min"][2:]))[0]
        hi = struct.unpack(">d", bytes.fromhex(a["max"][2:]))[0]
        if a["axis"] not in ctx:
            pending = pending or {"verdict": "not_checked", "axis": a["axis"], "reason": "not_observed"}
            continue
        x, u = ctx[a["axis"]]
        if u != a["unit"]:
            pending = pending or {"verdict": "not_checked", "axis": a["axis"], "reason": "unit_mismatch"}
            continue
        if x < lo:
            return {"verdict": "out_of_model_range", "axis": a["axis"], "side": "lower", "bound": a["min"],
                    "unit": a["unit"]}
        if x > hi:
            return {"verdict": "out_of_model_range", "axis": a["axis"], "side": "upper", "bound": a["max"],
                    "unit": a["unit"]}
    return pending or {"verdict": "in_range"}


RANGE_CASES = [
    ("ac4_52m", [("range", "52", "m")]),
    ("ac4_52m_in_temperature", [("range", "52", "m"), ("temperature", "293.15", "K")]),
    ("below_min", [("range", "0.4", "m"), ("temperature", "293.15", "K")]),
    ("at_min_closed", [("range", "0.5", "m"), ("temperature", "293.15", "K")]),
    ("at_max_closed", [("range", "40000", "mm"), ("temperature", "313.15", "K")]),
    ("just_above_max", [("range", "40.000001", "m"), ("temperature", "293.15", "K")]),
    ("in_range", [("range", "12.5", "m"), ("temperature", "293.15", "K")]),
    ("temperature_missing", [("range", "12.5", "m")]),
    ("temperature_too_hot", [("range", "12.5", "m"), ("temperature", "320", "K")]),
    ("range_in_seconds", [("range", "12.5", "s"), ("temperature", "293.15", "K")]),
    ("out_beats_unchecked", [("range", "12.5", "s"), ("temperature", "200", "K")]),
    ("extra_axis_ignored", [("range", "1", "m"), ("temperature", "300", "K"), ("elevation", "2", "rad")]),
]


# ------------------------------------------------------------------ outputs

def pretty(v):
    return (json.dumps(v, ensure_ascii=True, indent=2) + "\n").encode("utf-8")


def outputs():
    files = {}
    canon = {}
    for name, make in EXAMPLES.items():
        doc = make()
        files[name] = pretty(doc)
        canon[name] = canonical(canonical_model(doc))
    digests = {name: sha(b) for name, b in canon.items()}
    assert digests["tracker_example.json"] == digests["tracker_example_respelled.json"]
    assert digests["tracker_example.json"] != digests["tracker_example_conflict.json"]
    files["digests.txt"] = "".join("%s %s\n" % (n, digests[n]) for n in sorted(digests)).encode("ascii")
    files["tracker_example.canonical.json"] = canon["tracker_example.json"]

    labels = {}
    for name, (doc, code, field) in sorted(invalid().items()):
        files["invalid/" + name] = pretty(doc)
        labels[name] = {"code": code, "field": field}
    files["invalid/expected.json"] = pretty(labels)

    tdig = digests["tracker_example.json"]
    tlen = len(canon["tracker_example.json"])
    profile = standin_profile(tdig)
    instance = standin_instance()
    files["standin/sim-tracker-1.profile.json"] = pretty(profile)
    files["standin/sim-tracker-1.instance.json"] = pretty(instance)
    ddig = profile_digest(profile)
    idig = sha(canonical(instance))
    evidence = canonical({"descriptor": profile, "device": "sim-tracker-1", "instance": instance})
    edig = sha(evidence)

    model_id = "im:tracker-example@1.0.0"
    import_event = {
        "author_kind": "human", "blob": tdig, "blob_size": tlen, "branch": "main", "by": "alice",
        "command": "unc.import_instrument_model@1", "digest": tdig, "model": model_id,
    }
    bind_event = {
        "author_kind": "human", "branch": "main", "by": "alice", "command": "unc.bind_instrument_model@1",
        "descriptor_digest": ddig, "device": "sim-tracker-1", "device_instance_digest": idig,
        "evidence": edig, "evidence_size": len(evidence), "model": model_id, "model_digest": tdig,
        "target": "device",
    }
    project_event = {
        "author_kind": "human", "branch": "main", "by": "alice", "command": "unc.bind_instrument_model@1",
        "model": model_id, "model_digest": tdig, "target": "project",
    }
    tcanon = canonical_model(tracker())
    expected = {
        "tracker_digest": tdig,
        "tracker_canonical_size": tlen,
        "descriptor_digest": ddig,
        "device_instance_digest": idig,
        "evidence": evidence.decode("utf-8"),
        "evidence_digest": edig,
        "ac1_event": jcs(import_event),
        "ac1_result": jcs({"author_kind": "human", "digest": tdig, "model": model_id, "outcome": "stored"}),
        "ac1_again_result": jcs({"author_kind": "human", "digest": tdig, "model": model_id,
                                 "outcome": "already_stored"}),
        "dry_run_result": jcs({"author_kind": "human", "digest": tdig, "model": model_id, "outcome": "dry_run"}),
        "show_result": jcs({"author_kind": "human", "canonical": canon["tracker_example.json"].decode("utf-8"),
                            "digest": tdig, "model": model_id}),
        "ac3_event": jcs(bind_event),
        "ac3_result": jcs({"descriptor_digest": ddig, "device": "sim-tracker-1", "device_instance_digest": idig,
                           "model": model_id, "model_digest": tdig, "outcome": "bound", "target": "device"}),
        "ac3_again_result": jcs({"descriptor_digest": ddig, "device": "sim-tracker-1",
                                 "device_instance_digest": idig, "model": model_id, "model_digest": tdig,
                                 "outcome": "already_bound", "target": "device"}),
        "project_event": jcs(project_event),
        "project_result": jcs({"model": model_id, "model_digest": tdig, "outcome": "bound", "target": "project"}),
        "range_cases": [
            {"name": n, "context": [{"axis": a, "value": v, "unit": u} for a, v, u in ctx],
             "verdict": check_range(tcanon, ctx)}
            for n, ctx in RANGE_CASES
        ],
    }
    files["expected.json"] = pretty(expected)
    return files


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for rel, data in sorted(outputs().items()):
        path = os.path.join(HERE, *rel.split("/"))
        if check:
            try:
                with open(path, "rb") as f:
                    same = f.read() == data
            except OSError:
                same = False
            if not same:
                print("differs: " + rel)
                bad += 1
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(data)
    if check:
        print("ok" if bad == 0 else "%d file(s) differ" % bad)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
