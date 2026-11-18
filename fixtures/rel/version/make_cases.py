#!/usr/bin/env python3
"""Independent reference for ECO.REL.PROC.VERSION fixtures (CC-BY-4.0 project data).

Writes fixtures/rel/version/precedence.csv (a,b,expected: lt|eq|gt) and
fixtures/rel/version/tags.csv (tag,channel,expected: ok or an error code).

This script does not share code with the Rust implementation:
- tag form is decided by the spec's regular expression (REQ-ECO.REL.PROC.VERSION-001),
  plus the 64-byte bound and the integer widths the implementation documents
  (MAJOR/MINOR/PATCH fit in u64, the beta number fits in u32);
- dates are decided by Python's datetime.date (proleptic Gregorian), plus the rule
  that a year below 1000 is refused (its YYYYMMDD would start with 0, which SemVer
  forbids in a numeric identifier);
- precedence is a generic Semantic Versioning 2.0.0 comparison of dot-separated
  pre-release identifiers, not specialised to beta/nightly.

Run:  python fixtures/rel/version/make_cases.py          (rewrite the CSV files)
      python fixtures/rel/version/make_cases.py --check  (exit 1 if they are stale)
"""

import datetime
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
TAG_RE = re.compile(
    r"^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-beta\.[1-9][0-9]*|-nightly\.[0-9]{8})?$"
)
U64 = 2**64 - 1
U32 = 2**32 - 1


def classify(tag, channel):
    """Expected outcome of the tag check: form, then date, then channel suffix."""
    if len(tag.encode()) > 65 or not TAG_RE.fullmatch(tag):
        return "VERSION-TAG-FORMAT"
    core, _, pre = tag[1:].partition("-")
    if any(int(x) > U64 for x in core.split(".")):
        return "VERSION-TAG-FORMAT"
    if pre.startswith("beta.") and int(pre[5:]) > U32:
        return "VERSION-TAG-FORMAT"
    if pre.startswith("nightly."):
        d = pre[8:]
        try:
            datetime.date(int(d[:4]), int(d[4:6]), int(d[6:]))
        except ValueError:
            return "VERSION-DATE"
        if int(d[:4]) < 1000:
            return "VERSION-DATE"
    want = {"nightly": "nightly.", "beta": "beta.", "stable": None, "lts": None}[channel]
    if want is None and pre:
        return "VERSION-CHANNEL-SUFFIX"
    if want is not None and not pre.startswith(want):
        return "VERSION-CHANNEL-SUFFIX"
    return "ok"


def semver_cmp(a, b):
    """SemVer 2.0.0 precedence of a against b: -1, 0 or 1."""

    def split(v):
        core, _, pre = v.partition("-")
        return [int(x) for x in core.split(".")], (pre.split(".") if pre else [])

    (ca, pa), (cb, pb) = split(a), split(b)
    if ca != cb:
        return -1 if ca < cb else 1
    if not pa or not pb:
        return (len(pa) == 0) - (len(pb) == 0)
    for x, y in zip(pa, pb):
        if x == y:
            continue
        xn, yn = x.isdigit(), y.isdigit()
        if xn and yn:
            return -1 if int(x) < int(y) else 1
        if xn != yn:
            return -1 if xn else 1  # numeric identifiers are lower than alphanumeric ones
        return -1 if x < y else 1
    return (len(pa) > len(pb)) - (len(pa) < len(pb))


VERSIONS = [
    "0.0.1",
    "0.1.0",
    "1.3.9",
    "1.4.0-beta.1",
    "1.4.0-beta.2",
    "1.4.0-beta.10",
    "1.4.0-nightly.20261117",
    "1.4.0-nightly.20261201",
    "1.4.0",
    "1.4.1-beta.1",
    "1.4.1",
    "1.10.0",
    "2.0.0-nightly.20270101",
    "10.0.0",
]

TAGS = [
    ("v1.4.0-beta.2", "beta"),
    ("v1.4.0", "stable"),
    ("v1.4.0", "lts"),
    ("v0.0.0", "stable"),
    ("v1.4.0-nightly.20261117", "nightly"),
    ("v1.4.0-nightly.20240229", "nightly"),
    ("v1.4", "stable"),
    ("v01.4.0", "stable"),
    ("v1.04.0", "stable"),
    ("1.4.0", "stable"),
    ("V1.4.0", "stable"),
    ("v1.4.0.1", "stable"),
    ("v1.4.0-beta.0", "beta"),
    ("v1.4.0-beta.02", "beta"),
    ("v1.4.0-beta", "beta"),
    ("v1.4.0-rc.1", "beta"),
    ("v1.4.0-nightly.2026111", "nightly"),
    ("v1.4.0-nightly.202611170", "nightly"),
    ("v1.4.0+build.1", "stable"),
    ("v1.4.0 ", "stable"),
    ("v-1.4.0", "stable"),
    ("v18446744073709551615.0.0", "stable"),
    ("v18446744073709551616.0.0", "stable"),
    ("v1.4.0-beta.4294967295", "beta"),
    ("v1.4.0-beta.4294967296", "beta"),
    ("v1.4.0-beta.2", "stable"),
    ("v1.4.0-beta.2", "lts"),
    ("v1.4.0-beta.2", "nightly"),
    ("v1.4.0", "beta"),
    ("v1.4.0", "nightly"),
    ("v1.4.0-nightly.20261117", "beta"),
    ("v1.4.0-nightly.20261117", "stable"),
    ("v1.4.0-nightly.20261332", "nightly"),
    ("v1.4.0-nightly.20261300", "nightly"),
    ("v1.4.0-nightly.20261100", "nightly"),
    ("v1.4.0-nightly.20261131", "nightly"),
    ("v1.4.0-nightly.20250229", "nightly"),
    ("v1.4.0-nightly.21000229", "nightly"),
    ("v1.4.0-nightly.20000229", "nightly"),
    ("v1.4.0-nightly.09990101", "nightly"),
    ("v1.4.0-nightly.00000101", "nightly"),
    ("v1.4.0-nightly.20261332", "stable"),
    ("v" + "1" * 70 + ".0.0", "stable"),
]


def render():
    prec = ["a,b,expected"]
    word = {-1: "lt", 0: "eq", 1: "gt"}
    for a in VERSIONS:
        for b in VERSIONS:
            prec.append(f"{a},{b},{word[semver_cmp(a, b)]}")
    tags = ["tag,channel,expected"]
    for tag, channel in TAGS:
        assert "," not in tag and '"' not in tag
        tags.append(f"{tag},{channel},{classify(tag, channel)}")
    return {"precedence.csv": "\n".join(prec) + "\n", "tags.csv": "\n".join(tags) + "\n"}


def main():
    # Sanity checks of the reference against the spec's own examples (AC-2, AC-4, AC-6).
    assert semver_cmp("1.4.0-beta.2", "1.4.0") == -1
    assert semver_cmp("1.4.0-beta.10", "1.4.0-beta.2") == 1
    assert classify("v1.4", "stable") == "VERSION-TAG-FORMAT"
    assert classify("v01.4.0", "stable") == "VERSION-TAG-FORMAT"
    assert classify("v1.4.0-nightly.20261332", "nightly") == "VERSION-DATE"
    assert classify("v1.4.0-beta.2", "stable") == "VERSION-CHANNEL-SUFFIX"
    stale = False
    for name, text in render().items():
        path = HERE / name
        if "--check" in sys.argv:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"stale: {path}")
                stale = True
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    sys.exit(1 if stale else 0)


if __name__ == "__main__":
    main()
