#!/usr/bin/env python3
"""Independent reference for PLAT.MATRIX.FILE: writes fixtures/plat/matrix/expected.toml.

Reads the hand-written fixture files with Python's tomllib and applies the spec's rules
(NODE.md Requirements and Design) directly, without running or reading the Rust crate.
The Rust tests compare `fid_platform::matrix::validate(..).render()` and `Matrix::lookup` with
the values recorded here. Fixtures and this script: MIT OR Apache-2.0 (D-001).
Run:  python fixtures/plat/matrix/make_expected.py   (Python 3.11+)
"""

import json
import re
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OSES = ["windows", "ubuntu", "debian", "fedora", "rhel", "macos"]
ARCHES = ["x86_64", "aarch64"]
TIERS = ["tier1", "tier2", "tier3"]
PHASES = ["P0", "P1a", "P1b", "P2", "P3", "P4", "later"]  # CONSTITUTION section 7 order
ROW_KEYS = {"os", "version_min", "version_max", "arch", "tier", "since_phase", "notes"}
TOP_KEYS = {"schema_version", "row"}
MAX_NOTES = 4096
U32_MAX = 2**32 - 1

EXPECT = {
    "phase": "a phase code (P0, P1a, P1b, P2, P3, P4, later)",
    "os": "one of windows, ubuntu, debian, fedora, rhel, macos",
    "arch": "one of x86_64, aarch64",
    "tier": "one of tier1, tier2, tier3",
    "version": "a dotted numeric version of 1 to 4 u32 components",
}


def version(s):
    """Dotted numeric version -> 4-tuple padded with zeros, or None."""
    if not isinstance(s, str):
        return None
    parts = s.split(".")
    if not 1 <= len(parts) <= 4:
        return None
    out = []
    for p in parts:
        if not re.fullmatch(r"[0-9]{1,10}", p) or int(p) > U32_MAX:
            return None
        out.append(int(p))
    return tuple(out + [0] * (4 - len(out)))


def problem(row, key, allowed):
    """None if row[key] is a string in allowed; else 'is missing' / 'is not ...'."""
    if key not in row:
        return "is missing"
    v = row[key]
    return None if isinstance(v, str) and v in allowed else "invalid"


def validate(text):
    """Returns (rows, [diagnostic lines], [valid rows]). Fixtures here never hit the fatal size limits."""
    doc = tomllib.loads(text)
    errs = []
    sv = doc.get("schema_version")
    if "schema_version" not in doc:
        errs.append("BadField: schema_version is missing")
    elif isinstance(sv, int) and not isinstance(sv, bool) and sv >= 1:
        if sv > 1:
            return 0, [f"UnsupportedSchema: schema_version {sv} is above the supported version 1"], []
    else:
        errs.append("BadField: schema_version is not a positive integer")
    for k in sorted(doc):
        if k not in TOP_KEYS:
            errs.append(f"BadField: {k} is not a known key")
    rows = doc.get("row", [])
    if not isinstance(rows, list):
        errs.append("BadField: row is not an array of tables")
        rows = []
    good = []
    for n, r in enumerate(rows, start=1):
        if not isinstance(r, dict):
            errs.append(f"BadField: row {n}: row is not a table")
            continue
        mine = []
        for k in sorted(r):
            if k not in ROW_KEYS:
                mine.append(f"BadField: row {n}: {k} is not a known key")

        def enum(key, allowed, code, label, expect):
            p = problem(r, key, allowed)
            if p is not None:
                why = "is missing" if p == "is missing" else "is not " + EXPECT[expect]
                mine.append(f"{code}: row {n}: {label} {why}")

        enum("os", OSES, "UnknownOs", "os", "os")
        lo = version(r.get("version_min"))
        if "version_min" not in r:
            mine.append(f"BadVersion: row {n}: version_min is missing")
        elif lo is None:
            mine.append(f"BadVersion: row {n}: version_min is not {EXPECT['version']}")
        hi = None
        if "version_max" in r:
            hi = version(r["version_max"])
            if hi is None:
                mine.append(f"BadVersion: row {n}: version_max is not {EXPECT['version']}")
        enum("arch", ARCHES, "UnknownArch", "arch", "arch")
        enum("tier", TIERS, "BadTier", "tier", "tier")
        enum("since_phase", PHASES, "BadPhase", "since_phase", "phase")
        if "notes" not in r:
            mine.append(f"BadField: row {n}: notes is missing")
        elif not isinstance(r["notes"], str) or len(r["notes"].encode()) > MAX_NOTES:
            mine.append(f"BadField: row {n}: notes is not a string of at most 4096 bytes")
        if lo is not None and hi is not None and hi <= lo:
            mine.append(f"EmptyRange: row {n}: version_max is not above version_min")
        errs += mine
        if not mine:
            good.append({"n": n, "os": r["os"], "arch": r["arch"], "lo": lo, "hi": hi,
                         "tier": r["tier"], "since": r["since_phase"]})
    # Overlaps: every pair (no sorting shortcut), same os and arch, half-open ranges.
    pairs = []
    for i, a in enumerate(good):
        for b in good[i + 1:]:
            if a["os"] != b["os"] or a["arch"] != b["arch"]:
                continue
            a_before_b = a["hi"] is not None and a["hi"] <= b["lo"]
            b_before_a = b["hi"] is not None and b["hi"] <= a["lo"]
            if not (a_before_b or b_before_a):
                pairs.append((a["n"], b["n"], a["os"], a["arch"]))
    for a, b, os_, arch in sorted(pairs):
        errs.append(f"OverlappingRange: rows {a} and {b}: {os_} {arch} version ranges overlap")
    return len(rows), errs, good


def lookup(good, os_, ver, arch, phase):
    v = version(ver)
    if os_ not in OSES or arch not in ARCHES or v is None:
        return "not_listed"
    for r in good:
        if r["os"] == os_ and r["arch"] == arch and r["lo"] <= v and (r["hi"] is None or v < r["hi"]):
            if PHASES.index(phase) >= PHASES.index(r["since"]):
                return r["tier"]
            return f"planned({r['tier']}, {r['since']})"
    return "not_listed"


LOOKUPS = [
    # AC-4
    ("macos", "15.1", "aarch64", "P0"),
    ("macos", "15.1", "aarch64", "P1a"),
    ("freebsd", "14", "x86_64", "P1a"),
    # boundaries and phases
    ("macos", "15", "aarch64", "P2"),
    ("macos", "16", "aarch64", "P2"),
    ("macos", "15.7.3", "x86_64", "P4"),
    ("macos", "15.7.3", "x86_64", "later"),
    ("windows", "10.0.19045", "x86_64", "P0"),
    ("windows", "10.0.19045", "x86_64", "P1b"),
    ("windows", "10.0.22000", "x86_64", "P1a"),
    ("windows", "10.0.26100", "x86_64", "P3"),
    ("windows", "10.0.19040", "x86_64", "P3"),
    ("windows", "10.0.26100", "aarch64", "P3"),
    ("ubuntu", "24.04", "x86_64", "P0"),
    ("ubuntu", "24.04.1", "x86_64", "P0"),
    ("ubuntu", "24.4", "x86_64", "P0"),
    ("ubuntu", "24.10", "x86_64", "P0"),
    ("ubuntu", "24.04", "aarch64", "P1a"),
    ("ubuntu", "24.04", "aarch64", "P1b"),
    ("ubuntu", "24.04", "riscv64", "P1b"),
    ("ubuntu", "24..04", "x86_64", "P1b"),
    ("ubuntu", "", "x86_64", "P1b"),
    ("Ubuntu", "24.04", "x86_64", "P1b"),
    ("debian", "12", "x86_64", "P1b"),
]


def q(s):
    return json.dumps(s)  # ASCII JSON strings are valid TOML basic strings


def main():
    out = ["# Generated by make_expected.py (independent Python reference). Do not edit by hand.", ""]
    for name in ["valid.toml", "overlap.toml", "bad-phase.toml", "future-schema.toml", "errors.toml"]:
        rows, errs, _ = validate((HERE / name).read_text(encoding="utf-8"))
        out += ["[[validate]]", f"file = {q(name)}", f"rows = {rows}", "errors = ["]
        out += [f"  {q(e)}," for e in errs]
        out += ["]", ""]
    _, errs, good = validate((HERE / "valid.toml").read_text(encoding="utf-8"))
    assert not errs, errs
    for os_, ver, arch, phase in LOOKUPS:
        out += ["[[lookup]]", f"os = {q(os_)}", f"version = {q(ver)}", f"arch = {q(arch)}",
                f"phase = {q(phase)}", f"expect = {q(lookup(good, os_, ver, arch, phase))}", ""]
    (HERE / "expected.toml").write_text("\n".join(out), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
