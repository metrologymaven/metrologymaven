#!/usr/bin/env python3
"""Generate fixtures/knp1/find/ (spec node KRN.NUM.LINT.FIND, AC-1 .. AC-5).

This is the independent reference for the Rust crate `fid-knp1-lint`: the
expected reports are built here from the spec's rules (sort order, waiver
matching, LF-normalised line digests, RFC 8785 output), not by running Rust.

RFC 8785 note: every string here is ASCII and every number is a small
non-negative integer, so `json.dumps(sort_keys=True, separators=(",", ":"))`
is exactly the JCS form.

CC-BY-4.0 project data. Run:  python fixtures/knp1/find/make_fixtures.py
"""

import hashlib
import json
import random
from pathlib import Path

HERE = Path(__file__).resolve().parent
TOOL_VERSION = "0.0.1"


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def jcs(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def write(rel: str, data: bytes) -> None:
    p = HERE / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_bytes(data)


def finding(rule, crate, version, file, line, column, message, excerpt):
    return {"rule": rule, "crate_name": crate, "crate_version": version, "file": file, "line": line,
            "column": column, "excerpt_digest": sha(excerpt.encode()), "message": message}


def sort_key(f):
    # REQ-001: (crate, version, file, line, column, rule); message and excerpt break exact ties.
    return (f["crate_name"], f["crate_version"], f["file"], f["line"], f["column"], f["rule"], f["message"],
            f["excerpt_digest"])


def waiver_obj(w):
    return {"crate": w["crate"], "file": w["file"], "lines": [w["start"], w["end"]], "rule": w["rule"],
            "justification": w["justification"], "reviewer": w["reviewer"], "date": w["date"],
            "lines_sha256": w["lines_sha256"]}


def report(inputs, findings, waived=(), warnings=()):
    unwaived = sorted(findings, key=sort_key)
    by_rule = {}
    for f in unwaived:
        by_rule[f["rule"]] = by_rule.get(f["rule"], 0) + 1
    return {
        "schema": 1,
        "tool": "fid-knp1-lint",
        "tool_version": TOOL_VERSION,
        "inputs": inputs,
        "findings": unwaived,
        "waived": [{"finding": f, "waiver": waiver_obj(w)} for f, w in sorted(waived, key=lambda p: sort_key(p[0]))],
        "warnings": list(warnings),
        "summary": {"findings": len(unwaived), "waived": len(waived), "warnings": len(warnings), "by_rule": by_rule},
    }


INPUTS = {"config": sha(b"knp1-lint.toml fixture\n"), "lockfile": sha(b"Cargo.lock fixture\n"),
          "waivers": sha(b""), "denylist": sha(b"rdl-denylist.json fixture\n")}

# ---------------------------------------------------------------- AC-1: report-v1
F = [
    finding("KNP1-POW", "fid-clarabel", "0.1.0", "src/solver.rs", 11, 17, "powf call", "x.powf(0.5)"),
    finding("KNP1-FMA", "fid-clarabel", "0.1.0", "src/solver.rs", 11, 17, "mul_add call", "a.mul_add(b, c)"),
    finding("KNP1-MINMAX", "fid-clarabel", "0.1.0", "src/solver.rs", 11, 4, "f64::max call", "a.max(b)"),
    finding("KNP1-HASH", "fid-clarabel", "0.1.0", "src/cones/mod.rs", 200, 1, "std HashMap", "HashMap::new()"),
    finding("KNP1-UNSAFE", "fid-clarabel", "0.0.9", "src/solver.rs", 3, 1, "unsafe block", "unsafe {"),
    finding("KNP1-LIBM", "libm", "0.2.8", "src/math/exp.rs", 9, 12, "libm transcendental", "exp(x)"),
    finding("KNP1-IO-FS", "fid-num", "0.0.1", "src/lib.rs", 40, 5, "std::fs use", "std::fs::read"),
    finding("KNP1-CLOCK", "fid-num", "0.0.1", "src/lib.rs", 4, 5, "std::time use", "Instant::now()"),
    finding("KNP1-FLAG-TARGET-CPU", "fid-num", "0.0.1", "Cargo.toml", 1, 1, "target-cpu=native", "native"),
    finding("KNP1-ALLOW-LICENCE", "fid-clarabel", "0.1.0", "Cargo.toml", 7, 11, "licence not allowed", "GPL"),
    finding("KNP1-DATA-CONST", "fid-num", "0.0.1", "src/lib.rs", 40, 5, "reference constant", "3.14159265358979"),
    finding("KNP1-SIMD", "fid-kci", "0.0.1", "src/b/c.rs", 2, 9, "runtime CPU dispatch", "is_x86_feature_detected!"),
]
assert len(F) == 12
shuffled = list(F)
random.Random(20261117).shuffle(shuffled)
write("report-v1/input.json", jcs({"tool_version": TOOL_VERSION, "inputs": INPUTS, "findings": shuffled}))
write("report-v1/expected-report.json", jcs(report(INPUTS, F)))

# ---------------------------------------------------------------- AC-2 .. AC-5: waivers
SOLVER = [
    "//! Synthetic vendored solver source for the waiver fixtures.",
    "",
    "pub fn step(x: f64, y: f64) -> f64 {",
    "    let a = x * 2.0;",
    "    let b = y * 3.0;",
    "    let c = a + b;",
    "    let d = c - 1.0;",
    "    let e = d * d;",
    "    // Upstream Clarabel uses powf here; reviewed as bit-stable for this input range.",
    "    let s = e.sqrt();",
    "    let p = s.powf(0.5);",
    "    let q = p + 1.0;",
    "    q",
    "}",
    "",
    "pub fn other(z: f64) -> f64 {",
    "    let w = z + 1.0;",
    "    w * 2.0",
    "}",
    "",
    "pub fn tail() {}",
]
EDITED = list(SOLVER)
EDITED[10] = "    let p = s.powf(0.25);"  # line 11 edited -> waiver goes stale (AC-3)


def lines_digest(lines, start, end):
    # LF-normalised text of lines start..=end (1-based), each followed by "\n".
    return sha("".join(l + "\n" for l in lines[start - 1:end]).encode())


src = "\n".join(SOLVER) + "\n"
# The committed file is LF; the reader also accepts CRLF and normalises it.
write("src/fid-clarabel/src/solver.rs", src.encode())
write("src-edited/fid-clarabel/src/solver.rs", ("\n".join(EDITED) + "\n").encode())

POW_WAIVER = {"crate": "fid-clarabel", "file": "src/solver.rs", "start": 10, "end": 12, "rule": "KNP1-POW",
              "justification": "Vendored Clarabel powf; input range reviewed bit-stable.", "reviewer": "maintainer-a",
              "date": "2026-11-17", "lines_sha256": lines_digest(SOLVER, 10, 12)}
UNUSED_WAIVER = {"crate": "fid-clarabel", "file": "src/solver.rs", "start": 17, "end": 18, "rule": "KNP1-FMA",
                 "justification": "Kept for an upstream change that never landed.", "reviewer": "maintainer-b",
                 "date": "2026-11-16", "lines_sha256": lines_digest(SOLVER, 17, 18)}


def toml_waiver(w, drop=()):
    out = ["[[waiver]]"]
    for k, v in [("crate", w["crate"]), ("file", w["file"]), ("lines", [w["start"], w["end"]]), ("rule", w["rule"]),
                 ("justification", w["justification"]), ("reviewer", w["reviewer"]), ("date", w["date"]),
                 ("lines_sha256", w["lines_sha256"])]:
        if k in drop:
            continue
        out.append(f"{k} = [{v[0]}, {v[1]}]" if isinstance(v, list) else f'{k} = "{v}"')
    return "\n".join(out) + "\n"


HEADER = "# knp1-waivers.toml fixture (KRN.NUM.LINT.FIND). Generated by make_fixtures.py.\n\n"
valid = (HEADER + toml_waiver(POW_WAIVER)).encode()
unused = (HEADER + toml_waiver(POW_WAIVER) + "\n" + toml_waiver(UNUSED_WAIVER)).encode()
no_reviewer = (HEADER + toml_waiver(POW_WAIVER, drop=("reviewer",))).encode()
write("waivers/valid.toml", valid)
write("waivers/unused.toml", unused)
write("waivers/no-reviewer.toml", no_reviewer)

POW_FINDING = finding("KNP1-POW", "fid-clarabel", "0.1.0", "src/solver.rs", 11, 17, "powf call", "s.powf(0.5)")
# A finding outside the waived range (line 13) and one with another rule on line 11 stay unwaived in the
# extended case; the AC-2 case itself has only the waived finding so the unwaived count is 0.
write("waive-v1/findings.json", jcs([POW_FINDING]))
inputs_valid = dict(INPUTS, waivers=sha(valid))
write("waive-v1/expected-report.json", jcs(report(inputs_valid, [], waived=[(POW_FINDING, POW_WAIVER)])))

inputs_unused = dict(INPUTS, waivers=sha(unused))
warning = {"code": "KNP1-WAIVER-UNUSED", "waiver": waiver_obj(UNUSED_WAIVER)}
write("waive-v1/expected-report-unused.json",
      jcs(report(inputs_unused, [], waived=[(POW_FINDING, POW_WAIVER)], warnings=[warning])))

# Mixed case: one waived, two not (other rule on a waived line; same rule outside the range).
OTHER_RULE = finding("KNP1-FMA", "fid-clarabel", "0.1.0", "src/solver.rs", 11, 9, "mul_add call", "s.mul_add(1.0, 2.0)")
OUTSIDE = finding("KNP1-POW", "fid-clarabel", "0.1.0", "src/solver.rs", 13, 5, "powf call", "q.powf(2.0)")
OTHER_CRATE = finding("KNP1-POW", "fid-clarabel-ext", "0.1.0", "src/solver.rs", 11, 17, "powf call", "s.powf(0.5)")
mixed = [OUTSIDE, OTHER_CRATE, POW_FINDING, OTHER_RULE]
write("waive-v1/findings-mixed.json", jcs(mixed))
write("waive-v1/expected-report-mixed.json",
      jcs(report(inputs_valid, [OUTSIDE, OTHER_CRATE, OTHER_RULE], waived=[(POW_FINDING, POW_WAIVER)])))

# ---------------------------------------------------------------- schema negatives (tools/check_knp1_findings.py)
# Each is the AC-1 report with one hand-chosen violation of schema/knp1-findings-v1.json.
BASE = report(INPUTS, F)


def mutated(fn):
    r = json.loads(jcs(BASE))
    fn(r)
    return jcs(r)


NEG = {
    "schema-2": lambda r: r.__setitem__("schema", 2),
    "line-zero": lambda r: r["findings"][0].__setitem__("line", 0),
    "absolute-path": lambda r: r["findings"][0].__setitem__("file", "/home/ci/src/lib.rs"),
    "windows-path": lambda r: r["findings"][0].__setitem__("file", "src\\lib.rs"),
    "parent-path": lambda r: r["findings"][0].__setitem__("file", "../other/src/lib.rs"),
    "bad-rule": lambda r: r["findings"][0].__setitem__("rule", "KNP1-pow"),
    "missing-lockfile-digest": lambda r: r["inputs"].pop("lockfile"),
    "bad-digest": lambda r: r["inputs"].__setitem__("config", "sha256:ABC"),
    "extra-key": lambda r: r.__setitem__("generated_at", "2026-11-17T00:00:00Z"),
    "missing-summary": lambda r: r.pop("summary"),
}
for name, fn in NEG.items():
    write(f"schema-invalid/{name}.json", mutated(fn))

print("wrote fixtures under", HERE)
