#!/usr/bin/env python3
"""Independent reference for the seeded-violation fixtures of KRN.NUM.LINT.SYN (AC-1).

Each crate under fixtures/knp1/seeded/<RULE>/ plants exactly one KNP-1 violation. The
position of every planted violation is hand-labelled below as (line, token) from the rules
of spec/krn/num/lint/syn/NODE.md; this script finds the token on that line (it must occur
exactly once there), turns it into a 1-based character column, and computes the finding's
excerpt digest: SHA-256 of the line's text without its line ending. It writes
expected-findings.txt, one line per crate:

    RULE crate version file:line:column sha256:<hex>

Usage: python fixtures/knp1/seeded/make_expected.py [--check]
Stdlib only; it never runs the Rust code.
"""

import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
VERSION = "0.0.1"
FILE = "src/lib.rs"

# Hand labels: rule -> (line, token at the finding's column).
LABELS = {
    "KNP1-CLOCK": (4, "time"),
    "KNP1-ENV": (4, "env"),
    "KNP1-FMA": (4, "mul_add"),
    "KNP1-HASH": (4, "HashSet"),
    "KNP1-IO-FS": (4, "fs"),
    "KNP1-IO-NET": (4, "net"),
    "KNP1-LIBM": (4, "sqrt"),
    "KNP1-MINMAX": (4, "max"),
    "KNP1-POW": (4, "powi"),
    "KNP1-SIMD": (4, "is_x86_feature_detected"),
    "KNP1-THREAD": (4, "thread"),
    "KNP1-UNSAFE": (4, "unsafe"),
}


def expected() -> str:
    dirs = sorted(p.name for p in HERE.iterdir() if p.is_dir())
    if dirs != sorted(LABELS):
        raise SystemExit(f"seeded crates {dirs} do not match the labels {sorted(LABELS)}")
    rows = []
    for rule in dirs:
        line_no, token = LABELS[rule]
        text = (HERE / rule / FILE).read_bytes().decode("utf-8")
        lines = text.split("\n")
        line = lines[line_no - 1].removesuffix("\r")
        if line.count(token) != 1:
            raise SystemExit(f"{rule}: token {token!r} must occur exactly once on line {line_no}: {line!r}")
        col = len(line[: line.index(token)]) + 1
        digest = "sha256:" + hashlib.sha256(line.encode("utf-8")).hexdigest()
        crate = "seeded-" + rule.lower()
        rows.append((crate, f"{rule} {crate} {VERSION} {FILE}:{line_no}:{col} {digest}"))
    rows.sort()
    return "".join(r + "\n" for _, r in rows)


def main() -> int:
    out = HERE / "expected-findings.txt"
    text = expected()
    if "--check" in sys.argv[1:]:
        if not out.exists() or out.read_bytes() != text.encode("utf-8"):
            print(f"FAIL: {out} is stale; run without --check")
            return 1
        print("seeded expected-findings.txt: ok")
        return 0
    out.write_bytes(text.encode("utf-8"))
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
