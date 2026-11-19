#!/usr/bin/env python3
"""Independent reference for the KRN.NUM.LINT.DATA fixtures.

UNC.RDL.LINT (the denylist generator and its canary) is not built yet, so this script stands in
for it with the schema its Design pins: a hand-chosen set of reference values, dataset IDs and
file names is rendered into rdl-denylist.json (decimal renderings with 8 to 17 significant
digits plus the shortest round trip, the IEEE-754 hex literal, the excluded common constants,
and the SHA-256 digest of the RFC 8785 canonical JSON without `digest`). It also writes the
corrupt denylists, the canary crates, and the literal-forms file whose `//~` markers are the
hand-labelled expected findings (rule and token).

Usage: python fixtures/knp1/data/make_fixtures.py [--check]
Stdlib only; it never runs the Rust code.
"""

import hashlib
from decimal import Decimal
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

REFERENCE = [0.123456789012, 1234.56789012345]
IDS = ["tracim.lsq.circle.007"]
FILES = ["tracim-lsq-circle-007.json"]
EXCLUDED = ["0", "1", "-1", "0.5", "-0.5", "2", "-2"] + [f"1e{k}" for k in range(-12, 13)]


def hexlit(v: float) -> str:
    return "0x" + struct.pack(">d", v).hex()


def renderings(v: float) -> list:
    out = {repr(v)}
    for p in range(8, 18):
        out.add(f"{v:.{p - 1}e}")
    return sorted(out)


def canonical(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def denylist(corrupt_digest=False, drop=None) -> bytes:
    body = {
        "version": 1,
        "ids": sorted(IDS),
        "files": sorted(FILES),
        "values": sorted(({"decimal": renderings(v), "hex": hexlit(v)} for v in REFERENCE), key=lambda x: x["hex"]),
        "excluded": sorted(EXCLUDED),
    }
    digest = "sha256:" + hashlib.sha256(canonical(body)).hexdigest()
    if corrupt_digest:
        digest = digest[:-1] + ("0" if digest[-1] != "0" else "1")
    if drop:
        body.pop(drop)
    body["digest"] = digest
    return (json.dumps(body, sort_keys=True, indent=1) + "\n").encode("utf-8")


def cargo(name: str) -> bytes:
    return (
        f'[package]\nname = "{name}"\nversion = "0.0.1"\nedition = "2021"\n'
        'license = "MIT OR Apache-2.0"\npublish = false\n'
    ).encode("utf-8")


def forms() -> bytes:
    ref = REFERENCE[0]
    bits = struct.unpack(">Q", struct.pack(">d", ref))[0]
    h = hexlit(ref)
    # 20 significant digits of the double's exact value: not a denylisted rendering, same bits.
    near = str(Decimal(ref))[:22]
    assert float(near) == ref
    lines = [
        "//! KRN.NUM.LINT.DATA AC-2..AC-4: forms of the planted reference value 0.123456789012. A `//~`",
        "//! marker names each expected finding as RULE=token; other lines must yield nothing.",
        "pub const A: f64 = 0.123_456_789_012; //~ KNP1-DATA-VALUE=0.123_456_789_012",
        "pub const B: f64 = 0.123456789012f64; //~ KNP1-DATA-VALUE=0.123456789012f64",
        f"pub const C: f64 = f64::from_bits({h}); //~ KNP1-DATA-HEX={h}",
        "pub const D: f64 = 0.12345679; // AC-3: 8 significant digits and other bits",
        "pub const E: f64 = 1.23456789012e-1; //~ KNP1-DATA-VALUE=1.23456789012e-1",
        "pub const F: f64 = -0.123456789012; //~ KNP1-DATA-VALUE=0.123456789012",
        "pub const G: f64 = 0.1234567890120; //~ KNP1-DATA-VALUE=0.1234567890120",
        f"pub const H: f64 = {near}; //~ KNP1-DATA-HEX={near}",
        f"pub const I: u64 = {bits}; //~ KNP1-DATA-HEX={bits}",
        "pub const J: f64 = 1.0;",
        "pub const K: &str = \"data/TRACIM-LSQ-CIRCLE-007.JSON\"; //~ KNP1-DATA-FILE=\"data",
        "pub const L: &str = \"tracim.lsq.circle.007\"; //~ KNP1-DATA-ID=\"tracim",
        "pub const M: &str = concat!(\"tracim-lsq-\", \"circle-007.json\"); //~ KNP1-DATA-FILE=\"tracim",
        "pub const N: &str = \"tracim\\x2dlsq-circle-007.json\"; //~ KNP1-DATA-FILE=\"tracim",
        "pub const O: &[u8] = br\"tracim-lsq-circle-007.json\"; //~ KNP1-DATA-FILE=br\"",
        "pub const P: f64 = 1234.567_890_123_45; //~ KNP1-DATA-VALUE=1234.567_890_123_45",
        "pub const Q: f32 = 0.123456789012f32; //~ KNP1-DATA-VALUE=0.123456789012f32",
        "pub const R: f64 = 0.987654321098;",
        # Review of #245: padding and long exponents normalise before any length cap.
        "pub const S: f64 = 1234.56789012345e0000000000; //~ KNP1-DATA-VALUE=1234.56789012345e0",
        f"pub const T: f64 = 1234.56789012345{'0' * 1100}; //~ KNP1-DATA-VALUE=1234.56789012345000",
        f"pub const U: f64 = 1234.567{'_' * 1100}89012345; //~ KNP1-DATA-VALUE=1234.567__",
        "pub const V: f64 = 1e9999999999; //~ KNP1-PARSE=1e9999999999",
        # `concat!` renders integers, characters, floats and booleans too.
        "pub const W: &str = concat!(\"tracim-lsq-circle-00\", 7, \".json\"); //~ KNP1-DATA-FILE=\"tracim",
        "pub const X: &str = concat!(\"tracim-lsq-circle-007\", '.', \"json\"); //~ KNP1-DATA-FILE=\"tracim",
        "pub const Y: &str = concat!(\"tracim.lsq.circle.\", 0, 0, 7); //~ KNP1-DATA-ID=\"tracim",
        # Re-review of #245: `\u{…}` skips `_` without counting it and stops only at `}`.
        "pub const Z1: &str = \"tracim-lsq-circle-007\\u{2_________e}json\"; //~ KNP1-DATA-FILE=\"tracim",
        "pub const Z2: &str = \"tracim\\u{0_0_0_0_2_e}lsq\\u{2e}circle.007\"; //~ KNP1-DATA-ID=\"tracim",
        # A malformed escape is KNP1-PARSE, never matched as written.
        "pub const Z3: &str = \"tracim-lsq-circle-007.json \\u{110000}\"; //~ KNP1-PARSE=\"tracim",
        "pub const Z4: char = '\\u{d800}'; //~ KNP1-PARSE='",
        "macro_rules! planted {",
        "    () => {",
        "        0.123456789012 //~ KNP1-DATA-VALUE=0.123456789012",
        "    };",
        "}",
    ]
    return ("\n".join(lines) + "\n").encode("utf-8")


def outputs() -> dict:
    return {
        "denylist.json": denylist(),
        "bad-digest.json": denylist(corrupt_digest=True),
        "schema-no-ids.json": denylist(drop="ids"),
        "forms/Cargo.toml": cargo("knp1-data-forms"),
        "forms/src/lib.rs": forms(),
        "ws/Cargo.toml": b'[workspace]\nresolver = "2"\nmembers = ["canary"]\n',
        "ws/Cargo.lock": (
            b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\n"
            b'version = 4\n\n[[package]]\nname = "knp1-data-canary"\nversion = "0.0.1"\n'
        ),
        "ws/knp1-lint.toml": (
            b"# KRN.NUM.LINT.DATA canary fixture (stand-in for UNC.RDL.LINT's canary).\n"
            b'[graph]\nworkspace = "."\nroots = ["knp1-data-canary"]\n\n[data]\ndenylist = "../denylist.json"\n'
        ),
        "ws/canary/Cargo.toml": cargo("knp1-data-canary"),
        "ws/canary/src/lib.rs": b"//! Canary: one planted reference constant.\n\npub const R: f64 = 0.123456789012;\n",
        # A `\` + CRLF continuation inside a string (bytes kept by `-text`).
        "crlf/Cargo.toml": cargo("knp1-data-crlf"),
        "crlf/src/lib.rs": (
            b"//! Review of #245: CRLF line endings; the string continues across a CRLF.\r\n\r\n"
            b"pub const S: &str = \"tracim-lsq-\\\r\n    circle-007.json\";\r\n"
        ),
        "clean/Cargo.toml": cargo("knp1-data-canary"),
        "clean/src/lib.rs": b"//! Canary with the planted constant removed.\n\npub const R: f64 = 0.5;\n",
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for rel, data in outputs().items():
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != data:
                print(f"FAIL: {rel} is stale; run without --check")
                bad += 1
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    print("knp1 data fixtures:", "stale" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
