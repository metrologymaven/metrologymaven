#!/usr/bin/env python3
"""Independent reference for HAL.IDL.STAMP fixtures (CC-BY-4.0 project data).

Writes, next to this script:
  valid_linescan.json            an acquisition-batch/0 envelope with its stamp (AC-1, AC-2)
  valid_linescan.canonical.json  its RFC 8785 canonical bytes
  valid_linescan.sha256          sha256 of those bytes, as "sha256:<hex>" (AC-1, AC-2)
  missing_simulated.json         stamp without `simulated` (AC-3)
  bad_trust.json                 stamp with trust "verified" (AC-4)
  unknown_field.json             stamp with an extra field (AC-5)
  seq_reversed.json              stamp with seq ["200", "100"] (AC-6)
  claim_diameter.json            external-claim/0 referring to the batch above (AC-7)
  claim_diameter.canonical.json  its canonical bytes
  v1_major.json                  stamp declaring schema "stamp/1" (AC-8)
  invalid_cases.json             further rejected records with the expected code and pointer

Expected digests come from this script's own RFC 8785 writer and hashlib, never from the
Rust crate. Expected codes and pointers are hand-labelled from spec/hal/idl/stamp/NODE.md
and the wire rules documented in crates/fid-hal-idl/src/stamp/mod.rs:
  every field required, absent optional values spelled null, unknown fields refused;
  u64 / i64 as canonical decimal strings; the claim's f64 as "0x" + 16 hex digits of its bits;
  reference lists strictly ascending; the first offending field is the `schema` tag, then
  fields in sorted key order, depth first; cross-field rules (gaps, envelope vs stamp,
  compatibility status vs report digest, driver tier vs host digest) after that.

Fixture files are written with ensure_ascii=True and a two-space indent, so they are not
canonical and their non-ASCII text arrives as \\u escapes: the canonical form must re-encode
it as raw UTF-8.

Run:    python fixtures/hal/idl/stamp/make_fixtures.py
Check:  python fixtures/hal/idl/stamp/make_fixtures.py --check   (CI; exits 1 if any file differs)
"""

import copy
import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


# ------------------------------------------------------------------ RFC 8785 writer

def jcs(v):
    """RFC 8785 for the value shapes these records use (no JSON numbers appear)."""
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
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


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ------------------------------------------------------------------ building blocks

def uuid7(unix_ms, rand_a, rand_b):
    """RFC 9562 UUIDv7 from fixed fields: 48-bit ms, 12-bit rand_a, 62-bit rand_b."""
    n = (unix_ms << 80) | (0x7 << 76) | (rand_a << 64) | (0b10 << 62) | rand_b
    h = "%032x" % n
    return "%s-%s-%s-%s-%s" % (h[0:8], h[8:12], h[12:16], h[16:20], h[20:32])


def f64_bits(x):
    return "0x" + struct.pack(">d", x).hex()


def sha(label):
    return digest(label.encode("utf-8"))


BATCH_ID = uuid7(1763251200000, 0x0a5, 0x1d2c3b4a59687706)
assert BATCH_ID[14] == "7" and BATCH_ID[19] in "89ab"

SEQ = ["100", "1099"]
GAPS = [
    {"from": "250", "to": "251", "reason": "DEVICE_DROP"},
    {"from": "700", "to": "700", "reason": "CREDIT_EXHAUSTED"},
]
PAYLOAD_DIGEST = sha("fixture payload: line-scan profiles 100..1099")
PAYLOAD_SCHEMA = "line-scan-profile/1"


def stamp():
    return {
        "schema": "stamp/0",
        "batch_id": BATCH_ID,
        "seq": list(SEQ),
        "gaps": copy.deepcopy(GAPS),
        "payload_digest": PAYLOAD_DIGEST,
        "payload_schema": PAYLOAD_SCHEMA,
        "encoding_ref": "encoding:z-i32-scale-1e-7",
        "capability": {"name": "LineScan", "version": "1.2", "features": ["exposure_control", "intensity"]},
        "driver": {
            "id": "drv.fixture.linescan",
            "version": "0.4.1",
            "tier": "TIER2",
            "digest": sha("fixture driver binary"),
            "signer": "key:fixture-signer-2026",
            "host_digest": sha("fixture host binary"),
            "compat_report_digest": sha("fixture compatibility report"),
            "compat_status": "PASS",
        },
        "device": {
            # Non-ASCII on purpose (written as \u escapes in the fixture file).
            "make": "Fixture Optics GmbH",
            "model": "LS-7 µLine Ω",
            "serial": "SN-000123",
            "firmware": "fw 3.1.4 (build 2026-02-01)",
            "firmware_in_tested_range": True,
        },
        "config_digest": sha("fixture device configuration"),
        "calibration": {"status": "VALID", "refs": ["cal:fixture/2026-01-15", "cal:fixture/2026-03-02"]},
        "sensor_model": "sensor-model:fixture-ls7@1",
        "time": {
            "source": "PTP",
            "sync_sigma_ns": "250",
            "utc_anchor": {"t_host": "-42", "utc_ns": "1763251200000000000"},
        },
        "environment": ["env:fixture/air-temp-1", "env:fixture/part-temp-1"],
        "warmup": "READY",
        "transcript_digest": sha("fixture transcript segment"),
        "trust": "declared",
        "source_kind": "device",
        "simulated": False,
    }


def batch():
    return {
        "schema": "acquisition-batch/0",
        "batch_id": BATCH_ID,
        "payload_schema": PAYLOAD_SCHEMA,
        "payload_digest": PAYLOAD_DIGEST,
        "seq": list(SEQ),
        "gaps": copy.deepcopy(GAPS),
        "stamp": stamp(),
    }


def claim(batch_digest):
    return {
        "schema": "external-claim/0",
        "quantity": "diameter",
        "value": {"unit": "m", "value": f64_bits(0.0125)},
        "algorithm_source": "device",
        "method": "least-squares circle fit,\tfirmware édition 3 (device-supplied)",
        "batch": {"batch_id": BATCH_ID, "batch_digest": batch_digest},
    }


def without(d, key):
    d = copy.deepcopy(d)
    del d[key]
    return d


def with_(d, path, value):
    """Copy of d with the value at path (a list of keys / indices) replaced."""
    d = copy.deepcopy(d)
    cur = d
    for k in path[:-1]:
        cur = cur[k]
    cur[path[-1]] = value
    return d


# ------------------------------------------------------------------ invalid cases

def invalid_cases(batch_digest):
    s = stamp()
    b = batch()
    c = claim(batch_digest)
    cases = [
        # (name, record, code, pointer)
        ("not_an_object", ["stamp/0"], "stamp_value_invalid", ""),
        ("schema_missing", without(s, "schema"), "stamp_field_missing", "/schema"),
        ("schema_kind_unknown", with_(s, ["schema"], "stamps/0"), "stamp_enum_invalid", "/schema"),
        ("schema_not_tag", with_(s, ["schema"], "stamp"), "stamp_value_invalid", "/schema"),
        ("schema_v2_claim", with_(c, ["schema"], "external-claim/2"), "schema_version_unsupported", "/schema"),
        ("nested_stamp_v1", with_(b, ["stamp", "schema"], "stamp/1"), "schema_version_unsupported", "/stamp/schema"),
        ("nested_stamp_wrong_kind", with_(b, ["stamp", "schema"], "external-claim/0"), "stamp_enum_invalid",
         "/stamp/schema"),
        # Sorted-order rule: "batch_id" (missing) precedes "zz_extra" (unknown).
        ("first_in_sorted_order", with_(without(s, "batch_id"), ["zz_extra"], 1), "stamp_field_missing",
         "/batch_id"),
        ("unknown_nested", with_(s, ["driver", "licence"], "x"), "stamp_field_unknown", "/driver/licence"),
        ("unknown_key_escaped", with_(s, ["a/b~c"], True), "stamp_field_unknown", "/a~1b~0c"),
        ("trust_missing", without(s, "trust"), "stamp_field_missing", "/trust"),
        ("source_kind_missing", without(s, "source_kind"), "stamp_field_missing", "/source_kind"),
        ("source_kind_bad", with_(s, ["source_kind"], "simulator"), "stamp_enum_invalid", "/source_kind"),
        ("trust_not_string", with_(s, ["trust"], True), "stamp_enum_invalid", "/trust"),
        ("trust_upper", with_(s, ["trust"], "DECLARED"), "stamp_enum_invalid", "/trust"),
        ("simulated_null", with_(s, ["simulated"], None), "stamp_value_invalid", "/simulated"),
        ("simulated_string", with_(s, ["simulated"], "false"), "stamp_value_invalid", "/simulated"),
        ("gap_reason_bad", with_(s, ["gaps", 1, "reason"], "TIMEOUT"), "stamp_enum_invalid", "/gaps/1/reason"),
        ("calibration_status_bad", with_(s, ["calibration", "status"], "valid"), "stamp_enum_invalid",
         "/calibration/status"),
        ("warmup_bad", with_(s, ["warmup"], "COLD"), "stamp_enum_invalid", "/warmup"),
        ("time_source_bad", with_(s, ["time", "source"], "GPS"), "stamp_enum_invalid", "/time/source"),
        ("tier_bad", with_(s, ["driver", "tier"], "TIER4"), "stamp_enum_invalid", "/driver/tier"),
        ("seq_numbers", with_(s, ["seq"], [100, 1099]), "stamp_value_invalid", "/seq/0"),
        ("seq_three", with_(s, ["seq"], ["1", "2", "3"]), "stamp_value_invalid", "/seq"),
        ("seq_leading_zero", with_(s, ["seq"], ["0100", "1099"]), "stamp_value_invalid", "/seq/0"),
        ("seq_over_u64", with_(s, ["seq", 1], "18446744073709551616"), "stamp_value_invalid", "/seq/1"),
        ("gap_reversed", with_(s, ["gaps", 0], {"from": "252", "to": "250", "reason": "COMM_LOST"}),
         "stamp_seq_invalid", "/gaps/0"),
        ("gap_outside_range", with_(s, ["gaps", 1], {"from": "1099", "to": "1100", "reason": "COMM_LOST"}),
         "stamp_seq_invalid", "/gaps/1"),
        ("gaps_overlap", with_(s, ["gaps", 1], {"from": "251", "to": "260", "reason": "REJECTED"}),
         "stamp_seq_invalid", "/gaps/1"),
        ("gaps_too_many", with_(with_(s, ["seq"], ["0", "5000"]), ["gaps"],
                                [{"from": str(2 * i), "to": str(2 * i), "reason": "DEVICE_DROP"}
                                 for i in range(1025)]),
         "stamp_value_invalid", "/gaps"),
        ("digest_upper", with_(s, ["payload_digest"], PAYLOAD_DIGEST.upper().replace("SHA256", "sha256")),
         "stamp_value_invalid", "/payload_digest"),
        ("digest_md5", with_(s, ["config_digest"], "md5:" + "0" * 32), "stamp_value_invalid", "/config_digest"),
        ("batch_id_v4", with_(s, ["batch_id"], BATCH_ID[:14] + "4" + BATCH_ID[15:]), "stamp_value_invalid",
         "/batch_id"),
        ("batch_id_upper", with_(s, ["batch_id"], BATCH_ID.upper()), "stamp_value_invalid", "/batch_id"),
        ("payload_schema_bad", with_(s, ["payload_schema"], "LineScan"), "stamp_value_invalid", "/payload_schema"),
        ("capability_version_bad", with_(s, ["capability", "version"], "1.2.3"), "stamp_value_invalid",
         "/capability/version"),
        ("features_unsorted", with_(s, ["capability", "features"], ["intensity", "exposure_control"]),
         "stamp_value_invalid", "/capability/features/1"),
        ("features_duplicate", with_(s, ["capability", "features"], ["intensity", "intensity"]),
         "stamp_value_invalid", "/capability/features/1"),
        ("environment_too_many", with_(s, ["environment"], ["env:%03d" % i for i in range(65)]),
         "stamp_value_invalid", "/environment"),
        ("ref_with_space", with_(s, ["sensor_model"], "sensor model"), "stamp_value_invalid", "/sensor_model"),
        ("device_make_empty", with_(s, ["device", "make"], ""), "stamp_value_invalid", "/device/make"),
        ("device_model_too_long", with_(s, ["device", "model"], "é" * 257), "stamp_value_invalid",
         "/device/model"),
        ("device_serial_absent", with_(s, ["device"], without(s["device"], "serial")), "stamp_field_missing",
         "/device/serial"),
        ("sync_sigma_negative", with_(s, ["time", "sync_sigma_ns"], "-1"), "stamp_value_invalid",
         "/time/sync_sigma_ns"),
        ("utc_anchor_minus_zero", with_(s, ["time", "utc_anchor", "t_host"], "-0"), "stamp_value_invalid",
         "/time/utc_anchor/t_host"),
        ("compat_not_run_with_report", with_(s, ["driver", "compat_status"], "NOT_RUN"), "stamp_value_invalid",
         "/driver/compat_report_digest"),
        ("compat_pass_without_report", with_(s, ["driver", "compat_report_digest"], None), "stamp_value_invalid",
         "/driver/compat_report_digest"),
        # Tier 1 runs in-process, so a driver has a host digest exactly when it is not Tier 1.
        ("tier1_with_host_digest", with_(s, ["driver", "tier"], "TIER1"), "stamp_value_invalid",
         "/driver/host_digest"),
        ("tier2_without_host_digest", with_(s, ["driver", "host_digest"], None), "stamp_value_invalid",
         "/driver/host_digest"),
        ("batch_stamp_missing", without(b, "stamp"), "stamp_field_missing", "/stamp"),
        ("batch_nested_missing_simulated", with_(b, ["stamp"], without(s, "simulated")), "stamp_field_missing",
         "/stamp/simulated"),
        ("batch_nested_gap_reversed",
         with_(with_(b, ["stamp", "gaps", 0, "from"], "260"), ["gaps", 0, "from"], "260"),
         "stamp_seq_invalid", "/stamp/gaps/0"),
        ("batch_mismatch_payload_digest", with_(b, ["payload_digest"], sha("another payload")),
         "stamp_batch_mismatch", "/stamp/payload_digest"),
        ("batch_mismatch_gaps", with_(b, ["gaps"], []), "stamp_batch_mismatch", "/stamp/gaps"),
        ("batch_mismatch_seq", with_(b, ["seq"], ["100", "1100"]), "stamp_batch_mismatch", "/stamp/seq"),
        ("batch_seq_reversed", with_(b, ["seq"], ["200", "100"]), "stamp_seq_invalid", "/seq"),
        ("claim_value_of_record", with_(c, ["value_of_record"], True), "stamp_field_unknown", "/value_of_record"),
        ("claim_is_value_of_record", with_(c, ["is_value_of_record"], False), "stamp_field_unknown",
         "/is_value_of_record"),
        ("claim_source_platform", with_(c, ["algorithm_source"], "platform"), "stamp_enum_invalid",
         "/algorithm_source"),
        ("claim_unit_mm", with_(c, ["value", "unit"], "mm"), "stamp_enum_invalid", "/value/unit"),
        ("claim_value_nan", with_(c, ["value", "value"], "0x7ff8000000000000"), "stamp_value_invalid",
         "/value/value"),
        ("claim_value_inf", with_(c, ["value", "value"], "0x7ff0000000000000"), "stamp_value_invalid",
         "/value/value"),
        ("claim_value_number", with_(c, ["value", "value"], 0.0125), "stamp_value_invalid", "/value/value"),
        ("claim_method_too_long", with_(c, ["method"], "m" * 4097), "stamp_value_invalid", "/method"),
        ("claim_batch_unknown", with_(c, ["batch", "stamp"], {}), "stamp_field_unknown", "/batch/stamp"),
        ("claim_quantity_bad", with_(c, ["quantity"], "Diameter"), "stamp_value_invalid", "/quantity"),
    ]
    out = []
    for name, record, code, pointer in cases:
        out.append({"name": name, "record": record, "code": code, "pointer": pointer})
    return out


# ------------------------------------------------------------------ writing

OUTPUTS = {}


def write_json(name, value):
    write_bytes(name, (json.dumps(value, indent=2, ensure_ascii=True) + "\n").encode("ascii"))


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
    b = batch()
    b_canon = canonical(b)
    b_digest = digest(b_canon)
    write_json("valid_linescan.json", b)
    write_bytes("valid_linescan.canonical.json", b_canon)
    write_bytes("valid_linescan.sha256", (b_digest + "\n").encode("ascii"))

    write_json("missing_simulated.json", without(stamp(), "simulated"))
    write_json("bad_trust.json", with_(stamp(), ["trust"], "verified"))
    write_json("unknown_field.json", with_(stamp(), ["vendor_note"], "calibrated yesterday"))
    write_json("seq_reversed.json", with_(with_(stamp(), ["seq"], ["200", "100"]), ["gaps"], []))
    # A v1 stamp may carry fields v0 does not know; the version is reported, not the field.
    write_json("v1_major.json", with_(with_(stamp(), ["schema"], "stamp/1"), ["new_in_v1"], True))

    c = claim(b_digest)
    write_json("claim_diameter.json", c)
    write_bytes("claim_diameter.canonical.json", canonical(c))

    # One case per line keeps the file reviewable.
    cases = invalid_cases(b_digest)
    lines = ",\n".join("  " + json.dumps(case, ensure_ascii=True) for case in cases)
    write_bytes("invalid_cases.json", ("[\n" + lines + "\n]\n").encode("ascii"))
    status = flush(check)
    if status == 0:
        print("valid_linescan digest:", b_digest, "(checked)" if check else "(written)")
    return status


if __name__ == "__main__":
    sys.exit(main())
