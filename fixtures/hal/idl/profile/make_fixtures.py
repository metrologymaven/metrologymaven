#!/usr/bin/env python3
"""Independent reference for HAL.IDL.PROFILE fixtures (CC-BY-4.0 project data).

Every profile here is invented for testing: the makes, models, MPE values, source documents
and signatures are fictitious, and no maker data sheet or other third-party data is copied
(D-038). Standard references carry a designator, edition and clause only, never standards
text; the clause numbers are illustrative [unverified] and are not a mapping claim.

Writes, next to this script:
  arm_specsheet.json                  spec-sheet profile, uec_max 1 (AC-1)
  arm_specsheet.canonical.json        its RFC 8785 canonical bytes, signature included
  arm_specsheet.unsigned.json         canonical bytes with `signature` removed (digest preimage)
  arm_specsheet.sha256                sha256 of the unsigned bytes, as "sha256:<hex>" (AC-1)
  arm_specsheet_uec2.json             the same profile with uec_max 2 (AC-2)
  envelope_reversed.json              temperature 298.15 K .. 288.15 K, i.e. 25 C .. 15 C (AC-3)
  mpe_unit_um.json                    an MPE term in unit "um" (AC-4)
  cmm_iemps.json                      IEMPS profile, uec_max 2 (AC-5)
  cmm_iemps_resigned.json             the same profile with another signature block (AC-5)
  cmm_iemps.sha256                    digest of both (AC-5)
  valid_cases.json                    further accepted profiles with their expected digests
  invalid_cases.json                  further rejected profiles with expected code and pointer

Expected digests come from this script's own RFC 8785 writer and hashlib, never from the
Rust crate. Expected codes and pointers are hand-labelled from spec/hal/idl/profile/NODE.md
and the wire rules documented in crates/fid-hal-idl/src/profile/mod.rs:
  `schema` checked first ("instrument-profile/v<n>" with n != 1 is schema_version_unsupported);
  then, per object, the first unknown field in sorted key order, then the declared fields in
  sorted key order, depth first; the semantic rules run after the whole profile is
  structurally valid: validity envelope (range, then temperature), then the spec-sheet cap.
  Quantities are {"unit", "value"} with the value as "0x" + 16 hex digits of its f64 bits.

Fixture files are written with ensure_ascii=True and a two-space indent, so they are not
canonical and their non-ASCII text arrives as \\u escapes.

Run:    python fixtures/hal/idl/profile/make_fixtures.py
Check:  python fixtures/hal/idl/profile/make_fixtures.py --check   (CI; exits 1 if any file differs)
"""

import base64
import copy
import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


# ------------------------------------------------------------------ RFC 8785 writer

def jcs(v):
    """RFC 8785 for the value shapes profiles use (the only JSON numbers are small integers)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        return str(v)
    if isinstance(v, str):
        # Python escapes exactly '"', '\\' and U+0000..U+001F (short forms for \b \t \n
        # \f \r, lowercase \u00xx otherwise) when ensure_ascii=False, as RFC 8785 does.
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


def label_digest(label):
    return sha(label.encode("utf-8"))


def unsigned(profile):
    p = copy.deepcopy(profile)
    del p["signature"]
    return p


def profile_digest(profile):
    """REQ-006: SHA-256 over the canonical JSON of the profile without its signature block."""
    return sha(canonical(unsigned(profile)))


# ------------------------------------------------------------------ building blocks

def bits(x):
    return "0x" + struct.pack(">d", x).hex()


def q(value, unit):
    return {"unit": unit, "value": bits(value)}


def raw_q(value_bits, unit):
    return {"unit": unit, "value": value_bits}


def std(designator, edition, clause):
    return {"designator": designator, "edition": edition, "clause": clause}


def sig(keyid, text):
    return {"keyid": keyid, "sig": base64.b64encode(text.encode("utf-8")).decode("ascii")}


def kelvin(celsius):
    return celsius + 273.15


ARM_STD = std("ISO 10360-12", "2016", "6.2")  # illustrative clause [unverified]


def arm():
    """A fictitious articulated arm described from an invented spec sheet."""
    return {
        "schema": "instrument-profile/v1",
        "profile_id": "arm-demo-01@1.0.0",
        "device_class": ["PortablePose", "TactileProbe"],
        # Non-ASCII on purpose (written as \u escapes in the fixture file).
        "make": "Example Arm Works (invented)",
        "model": "DemoArm 7 µ Ω",
        "mpe_terms": [
            {"standard": ARM_STD, "symbol": "E_UNI", "value": q(45e-6, "m"),
             "contributor": "uc:arm-length-error@1.0.0"},
            {"standard": ARM_STD, "symbol": "P_SIZE", "value": q(20e-6, "m"),
             "contributor": "uc:arm-probing-size@1.0.0"},
            {"standard": ARM_STD, "symbol": "P_FORM", "value": q(30e-6, "m"),
             "contributor": "uc:arm-probing-form@1.0.0"},
        ],
        "frame_chain": [
            {"name": "probe-tip", "kind": "sensor"},
            {"name": "probe-mount", "kind": "mount"},
            {"name": "arm-base", "kind": "device"},
        ],
        "validity": {
            "range": {"lower": q(0.0, "m"), "upper": q(1.2, "m")},
            "temperature": {"lower": q(kelvin(15.0), "K"), "upper": q(kelvin(30.0), "K")},
        },
        "source": {
            "kind": "spec-sheet",
            "digest": label_digest("invented spec sheet: DemoArm 7, fixture only"),
            "citation": "Invented example spec sheet for a fictitious arm; not a real product "
                        "(project fixture, CC-BY-4.0)",
        },
        "um_model": {"id": "im:arm-demo-01@1.0.0", "digest": label_digest("fixture model im:arm-demo-01@1.0.0")},
        "uec_max": 1,
        "signature": sig("key:fixture-profile-signer-2026", "fixture signature bytes, not a real signature"),
    }


CMM_STD = std("ISO 10360-2", "2009", "6")  # illustrative clause [unverified]


def cmm():
    """A fictitious bridge CMM described by an invented IEMPS parameter set."""
    return {
        "schema": "instrument-profile/v1",
        "profile_id": "cmm-demo-02@2.1.0",
        "device_class": ["CncMotion", "Rotary", "TactileProbe"],
        "make": "Example Bridge Co. (invented)",
        "model": "DemoBridge 1000",
        "mpe_terms": [
            {"standard": CMM_STD, "symbol": "E0", "value": q(1.5e-6, "m"),
             "contributor": "uc:cmm-length-error@1.0.0"},
            {"standard": CMM_STD, "symbol": "E0.L", "value": q(2.5e-6, "1"),
             "contributor": "uc:cmm-length-error-slope@1.0.0"},
        ],
        "frame_chain": [
            {"name": "stylus-tip", "kind": "sensor"},
            {"name": "probe-head", "kind": "mount"},
            {"name": "ram", "kind": "device"},
            {"name": "table", "kind": "machine"},
        ],
        "validity": {
            "range": {"lower": q(0.0, "m"), "upper": q(1.0, "m")},
            "temperature": {"lower": q(kelvin(18.0), "K"), "upper": q(kelvin(22.0), "K")},
        },
        "source": {
            "kind": "iemps",
            "digest": label_digest("invented IEMPS parameter set: DemoBridge 1000, fixture only"),
            "citation": "Invented IEMPS parameter set for a fictitious CMM (project fixture, CC-BY-4.0)",
        },
        "um_model": {"id": "im:cmm-demo-02@2.1.0", "digest": label_digest("fixture model im:cmm-demo-02@2.1.0")},
        "uec_max": 2,
        "signature": sig("key:fixture-profile-signer-2026", "fixture signature over cmm-demo-02"),
    }


def put(doc, path, value):
    d = copy.deepcopy(doc)
    cur = d
    for k in path[:-1]:
        cur = cur[k]
    cur[path[-1]] = value
    return d


def drop(doc, path):
    d = copy.deepcopy(doc)
    cur = d
    for k in path[:-1]:
        cur = cur[k]
    del cur[path[-1]]
    return d


# ------------------------------------------------------------------ cases

def valid_cases():
    a = arm()
    c = cmm()
    cases = [
        ("make_exactly_4096_bytes", put(a, ["make"], "a" * 4096)),
        ("make_4096_bytes_multibyte", put(a, ["make"], "é" * 2048)),
        ("range_lower_equals_upper", put(a, ["validity", "range", "upper"], q(0.0, "m"))),
        ("temperature_lower_equals_upper",
         put(a, ["validity", "temperature"], {"lower": q(kelvin(20.0), "K"), "upper": q(kelvin(20.0), "K")})),
        ("range_in_radians", put(a, ["validity", "range"], {"lower": q(-3.0, "rad"), "upper": q(3.0, "rad")})),
        ("specsheet_uec0", put(a, ["uec_max"], 0)),
        ("acceptance_certificate_uec2", put(put(a, ["source", "kind"], "acceptance-certificate"), ["uec_max"], 2)),
        ("iemps_uec3", put(c, ["uec_max"], 3)),
        ("iemps_no_mpe_terms", put(c, ["mpe_terms"], [])),
        ("mpe_value_zero", put(a, ["mpe_terms", 0, "value"], q(0.0, "m"))),
        ("single_sensor_frame", put(a, ["frame_chain"], [{"name": "probe-tip", "kind": "sensor"}])),
        ("two_mount_frames", put(a, ["frame_chain"], [
            {"name": "probe-tip", "kind": "sensor"},
            {"name": "adapter", "kind": "mount"},
            {"name": "extension", "kind": "mount"},
            {"name": "arm-base", "kind": "device"},
        ])),
        ("prerelease_semver", put(a, ["profile_id"], "arm-demo-01@1.0.0-rc.1")),
        ("control_characters_in_untrusted_text", put(a, ["model"], "DemoArm‮ 7\n")),
        ("other_signature_same_digest", put(a, ["signature"], sig("key:another-signer", "another fixture signature"))),
    ]
    return [{"case": name, "digest": profile_digest(doc), "profile": doc} for name, doc in cases]


def invalid_cases():
    a = arm()
    c = cmm()
    V = "profile_value_invalid"
    E = "profile_envelope_invalid"
    cases = [
        # Schema tag, checked first.
        ("schema_v2", put(put(a, ["schema"], "instrument-profile/v2"), ["new_in_v2"], True),
         "schema_version_unsupported", "/schema"),
        ("schema_other_record", put(a, ["schema"], "stamp/0"), V, "/schema"),
        ("schema_missing", drop(a, ["schema"]), "profile_field_missing", "/schema"),
        ("not_an_object", ["instrument-profile/v1"], V, ""),
        # Unknown and missing fields (REQ-001).
        ("unknown_top_level_field", put(a, ["vendor_note"], "calibrated yesterday"), "profile_field_unknown",
         "/vendor_note"),
        ("standards_text_field", put(a, ["mpe_terms", 0, "standard", "text"], "pasted clause text"),
         "profile_field_unknown", "/mpe_terms/0/standard/text"),
        ("quantity_extra_field", put(a, ["validity", "range", "lower", "scale"], "0x3ff0000000000000"),
         "profile_field_unknown", "/validity/range/lower/scale"),
        ("make_missing", drop(a, ["make"]), "profile_field_missing", "/make"),
        ("signature_missing", drop(a, ["signature"]), "profile_field_missing", "/signature"),
        ("source_citation_missing", drop(a, ["source", "citation"]), "profile_field_missing", "/source/citation"),
        ("mpe_contributor_missing", drop(a, ["mpe_terms", 1, "contributor"]), "profile_field_missing",
         "/mpe_terms/1/contributor"),
        # First error in sorted key order: device_class comes before make.
        ("two_errors_sorted_order", drop(put(a, ["device_class"], []), ["make"]), V, "/device_class"),
        # Structural errors win over semantic rules.
        ("structural_before_semantic",
         drop(put(a, ["validity", "temperature"], {"lower": q(kelvin(25.0), "K"), "upper": q(kelvin(15.0), "K")}),
              ["model"]),
         "profile_field_missing", "/model"),
        # profile_id
        ("profile_id_no_version", put(a, ["profile_id"], "arm-demo-01"), V, "/profile_id"),
        ("profile_id_short_version", put(a, ["profile_id"], "arm-demo-01@1.0"), V, "/profile_id"),
        ("profile_id_upper_case", put(a, ["profile_id"], "Arm-Demo@1.0.0"), V, "/profile_id"),
        ("profile_id_leading_zero", put(a, ["profile_id"], "arm-demo-01@01.0.0"), V, "/profile_id"),
        # device_class
        ("device_class_empty", put(a, ["device_class"], []), V, "/device_class"),
        ("device_class_unsorted", put(a, ["device_class"], ["TactileProbe", "PortablePose"]), V, "/device_class"),
        ("device_class_duplicate", put(a, ["device_class"], ["PortablePose", "PortablePose"]), V, "/device_class"),
        ("device_class_bad_name", put(a, ["device_class"], ["lineScan"]), V, "/device_class/0"),
        ("device_class_not_string", put(a, ["device_class"], ["PortablePose", 7]), V, "/device_class/1"),
        # Untrusted text (AC-6 boundary, REQ-001).
        ("make_4097_bytes", put(a, ["make"], "a" * 4097), "untrusted_text_too_long", "/make"),
        ("model_4098_bytes_multibyte", put(a, ["model"], "é" * 2049), "untrusted_text_too_long", "/model"),
        ("citation_too_long", put(a, ["source", "citation"], "c" * 4097), "untrusted_text_too_long",
         "/source/citation"),
        ("make_empty", put(a, ["make"], ""), V, "/make"),
        ("model_not_string", put(a, ["model"], 7), V, "/model"),
        # MPE terms (REQ-002).
        ("mpe_unit_mm", put(a, ["mpe_terms", 0, "value", "unit"], "mm"), "unit_unsupported",
         "/mpe_terms/0/value/unit"),
        ("mpe_unit_and_nan", put(put(a, ["mpe_terms", 0, "value", "unit"], "um"),
                                 ["mpe_terms", 0, "value", "value"], "0x7ff8000000000000"),
         "unit_unsupported", "/mpe_terms/0/value/unit"),
        ("mpe_value_nan", put(a, ["mpe_terms", 0, "value"], raw_q("0x7ff8000000000000", "m")), "quantity_not_finite",
         "/mpe_terms/0/value/value"),
        ("mpe_value_infinite", put(a, ["mpe_terms", 2, "value"], raw_q("0x7ff0000000000000", "m")),
         "quantity_not_finite", "/mpe_terms/2/value/value"),
        ("mpe_value_negative", put(a, ["mpe_terms", 0, "value"], q(-45e-6, "m")), V, "/mpe_terms/0/value/value"),
        ("mpe_value_negative_zero", put(a, ["mpe_terms", 0, "value"], q(-0.0, "m")), V, "/mpe_terms/0/value/value"),
        ("mpe_value_decimal_number", put(a, ["mpe_terms", 0, "value", "value"], "4.5e-05"), V,
         "/mpe_terms/0/value/value"),
        ("mpe_contributor_no_prefix", put(a, ["mpe_terms", 0, "contributor"], "arm-length-error@1.0.0"), V,
         "/mpe_terms/0/contributor"),
        ("mpe_contributor_model_prefix", put(a, ["mpe_terms", 0, "contributor"], "im:arm-length-error@1.0.0"), V,
         "/mpe_terms/0/contributor"),
        ("mpe_symbol_empty", put(a, ["mpe_terms", 0, "symbol"], ""), V, "/mpe_terms/0/symbol"),
        ("mpe_symbol_with_space", put(a, ["mpe_terms", 0, "symbol"], "E UNI"), V, "/mpe_terms/0/symbol"),
        ("designator_too_long", put(a, ["mpe_terms", 0, "standard", "designator"], "D" * 65), V,
         "/mpe_terms/0/standard/designator"),
        ("clause_with_newline", put(a, ["mpe_terms", 0, "standard", "clause"], "6.2\nshall"), V,
         "/mpe_terms/0/standard/clause"),
        ("edition_empty", put(a, ["mpe_terms", 0, "standard", "edition"], ""), V, "/mpe_terms/0/standard/edition"),
        ("mpe_terms_not_list", put(a, ["mpe_terms"], {}), V, "/mpe_terms"),
        ("mpe_terms_too_many", put(a, ["mpe_terms"], [a["mpe_terms"][0]] * 65), V, "/mpe_terms"),
        # Frame chain.
        ("frame_chain_empty", put(a, ["frame_chain"], []), V, "/frame_chain"),
        ("frame_chain_starts_at_mount", put(a, ["frame_chain"], a["frame_chain"][1:]), V, "/frame_chain/0/kind"),
        ("frame_chain_goes_back", put(a, ["frame_chain", 2, "kind"], "sensor"), V,
         "/frame_chain/2/kind"),
        ("frame_chain_part", put(a, ["frame_chain", 2, "kind"], "part"), V, "/frame_chain/2/kind"),
        ("frame_chain_unknown_kind", put(a, ["frame_chain", 1, "kind"], "probe"), V, "/frame_chain/1/kind"),
        ("frame_chain_duplicate_name", put(a, ["frame_chain", 1, "name"], "probe-tip"), V, "/frame_chain/1/name"),
        ("frame_chain_bad_name", put(a, ["frame_chain", 0, "name"], "Probe Tip"), V, "/frame_chain/0/name"),
        ("frame_chain_too_long", put(a, ["frame_chain"],
                                     [{"name": "f%02d" % i, "kind": "sensor"} for i in range(17)]),
         V, "/frame_chain"),
        # Validity envelope (REQ-005).
        ("range_reversed", put(a, ["validity", "range"], {"lower": q(1.2, "m"), "upper": q(0.0, "m")}), E,
         "/validity/range"),
        ("range_mixed_units", put(a, ["validity", "range", "upper"], q(1.2, "rad")), E, "/validity/range"),
        ("range_in_seconds", put(a, ["validity", "range"], {"lower": q(0.0, "s"), "upper": q(1.0, "s")}), E,
         "/validity/range"),
        ("temperature_in_pascal", put(a, ["validity", "temperature", "lower"], q(kelvin(15.0), "Pa")), E,
         "/validity/temperature"),
        ("temperature_in_celsius_numbers",
         put(a, ["validity", "temperature"], {"lower": q(-5.0, "K"), "upper": q(30.0, "K")}), E,
         "/validity/temperature"),
        ("temperature_unit_degc", put(a, ["validity", "temperature", "lower", "unit"], "degC"), "unit_unsupported",
         "/validity/temperature/lower/unit"),
        ("range_lower_nan", put(a, ["validity", "range", "lower"], raw_q("0x7ff8000000000000", "m")),
         "quantity_not_finite", "/validity/range/lower/value"),
        # Range is checked before temperature; the envelope before the spec-sheet cap.
        ("range_and_temperature_reversed",
         put(put(a, ["validity", "range"], {"lower": q(1.2, "m"), "upper": q(0.0, "m")}),
             ["validity", "temperature"], {"lower": q(kelvin(25.0), "K"), "upper": q(kelvin(15.0), "K")}),
         E, "/validity/range"),
        ("envelope_before_uec_cap",
         put(put(a, ["validity", "temperature"], {"lower": q(kelvin(25.0), "K"), "upper": q(kelvin(15.0), "K")}),
             ["uec_max"], 2),
         E, "/validity/temperature"),
        # Source and the spec-sheet cap (REQ-003, REQ-004).
        ("source_kind_unknown", put(a, ["source", "kind"], "datasheet"), V, "/source/kind"),
        ("source_kind_underscore", put(a, ["source", "kind"], "spec_sheet"), V, "/source/kind"),
        ("source_digest_upper_case", put(a, ["source", "digest"], a["source"]["digest"].upper()), V,
         "/source/digest"),
        ("source_digest_sha1", put(a, ["source", "digest"], "sha1:" + "0" * 40), V, "/source/digest"),
        ("specsheet_uec3", put(a, ["uec_max"], 3), "profile_uec_exceeds_source", "/uec_max"),
        ("uec_above_3", put(c, ["uec_max"], 4), V, "/uec_max"),
        ("uec_negative", put(c, ["uec_max"], -1), V, "/uec_max"),
        ("uec_string", put(a, ["uec_max"], "1"), V, "/uec_max"),
        ("uec_label", put(a, ["uec_max"], "UEC-1"), V, "/uec_max"),
        # Model reference.
        ("um_model_wrong_prefix", put(a, ["um_model", "id"], "uc:arm-demo-01@1.0.0"), V, "/um_model/id"),
        ("um_model_no_version", put(a, ["um_model", "id"], "im:arm-demo-01"), V, "/um_model/id"),
        ("um_model_digest_missing", drop(a, ["um_model", "digest"]), "profile_field_missing", "/um_model/digest"),
        ("um_model_as_string", put(a, ["um_model"], "im:arm-demo-01@1.0.0@" + a["um_model"]["digest"]), V,
         "/um_model"),
        # Signature block: present and well formed only (TRUST verifies it).
        ("signature_not_object", put(a, ["signature"], "unsigned"), V, "/signature"),
        ("signature_null", put(a, ["signature"], None), V, "/signature"),
        ("signature_not_base64", put(a, ["signature", "sig"], "not base64!"), V, "/signature/sig"),
        ("signature_bad_padding", put(a, ["signature", "sig"], "QUJD="), V, "/signature/sig"),
        ("signature_empty", put(a, ["signature", "sig"], ""), V, "/signature/sig"),
        ("signature_keyid_empty", put(a, ["signature", "keyid"], ""), V, "/signature/keyid"),
        ("signature_extra_field", put(a, ["signature", "alg"], "ed25519"), "profile_field_unknown",
         "/signature/alg"),
    ]
    return [{"case": name, "code": code, "pointer": ptr, "profile": doc} for name, doc, code, ptr in cases]


# ------------------------------------------------------------------ output

OUTPUTS = {}


def write_json(name, value):
    text = json.dumps(value, ensure_ascii=True, indent=2) + "\n"
    OUTPUTS[name] = text.encode("ascii")


def write_bytes(name, data):
    OUTPUTS[name] = data


def write_lines(name, rows):
    # One case per line keeps the file reviewable.
    lines = ",\n".join("  " + json.dumps(row, ensure_ascii=True) for row in rows)
    write_bytes(name, ("[\n" + lines + "\n]\n").encode("ascii"))


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
    a = arm()
    a_digest = profile_digest(a)
    write_json("arm_specsheet.json", a)
    write_bytes("arm_specsheet.canonical.json", canonical(a))
    write_bytes("arm_specsheet.unsigned.json", canonical(unsigned(a)))
    write_bytes("arm_specsheet.sha256", (a_digest + "\n").encode("ascii"))
    write_json("arm_specsheet_uec2.json", put(a, ["uec_max"], 2))
    write_json("envelope_reversed.json",
               put(a, ["validity", "temperature"], {"lower": q(kelvin(25.0), "K"), "upper": q(kelvin(15.0), "K")}))
    write_json("mpe_unit_um.json", put(a, ["mpe_terms", 0, "value", "unit"], "um"))

    c = cmm()
    c_digest = profile_digest(c)
    resigned = put(c, ["signature"], sig("key:fixture-profile-signer-2027", "a different fixture signature"))
    assert profile_digest(resigned) == c_digest and canonical(resigned) != canonical(c)
    write_json("cmm_iemps.json", c)
    write_json("cmm_iemps_resigned.json", resigned)
    write_bytes("cmm_iemps.sha256", (c_digest + "\n").encode("ascii"))

    write_lines("valid_cases.json", valid_cases())
    write_lines("invalid_cases.json", invalid_cases())
    status = flush(check)
    if status == 0:
        verb = "(checked)" if check else "(written)"
        print("arm_specsheet digest:", a_digest, verb)
        print("cmm_iemps digest:", c_digest, verb)
    return status


if __name__ == "__main__":
    sys.exit(main())
