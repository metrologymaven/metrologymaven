#!/usr/bin/env python3
"""Independent reference for the ECO.CI.GATES planner (`cargo xtask ci-plan`).

Writes fixtures/gates/expected.json: for every planner case (a config and a changed-path
list, both hand-written files in this folder) the expected exit code, the expected
`--json` and text output, or the expected error code; plus glob-match cases and globs the
planner must refuse. The rules are taken from spec/eco/ci/gates/NODE.md (REQ-001..004 and
the Design); the glob matcher here translates each glob to a Python regular expression,
a different technique from the Rust automaton, so the two check each other.

Stdlib only (Python 3.11+ for tomllib). MIT OR Apache-2.0 project data.
Run:  python fixtures/gates/make_cases.py          (rewrite expected.json)
      python fixtures/gates/make_cases.py --check  (fail if expected.json is stale)
"""

import json
import re
import sys
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "expected.json"

MAX_CONFIG_BYTES = 64 * 1024
MAX_CHANGED_PATHS = 100_000
MAX_PATH_BYTES = 4096
MAX_GLOB_BYTES = 256
DEFAULT_CI = [".github/workflows/**", ".github/actions/**", "ci/**"]
DEFAULT_TOOLS = ["xtask/**"]
ID_RE = re.compile(r"[a-z][a-z0-9-]*\Z")


class PlanError(Exception):
    def __init__(self, code, detail):
        super().__init__(detail)
        self.code = code


# --- glob -> regex ---------------------------------------------------------------------

def glob_regex(g):
    """Returns a compiled regex for glob g, or raises ValueError if g is not allowed."""
    if not g or len(g.encode()) > MAX_GLOB_BYTES:
        raise ValueError("length")
    if any(ord(c) < 0x20 or 0x7F <= ord(c) <= 0x9F or c in "\\[]{}" for c in g):
        raise ValueError("character")
    if g.startswith("!") or g.startswith("/") or g.endswith("/"):
        raise ValueError("anchor")
    segs = g.split("/")
    parts = []
    for i, s in enumerate(segs):
        last = i == len(segs) - 1
        if s in ("", ".", ".."):
            raise ValueError("segment")
        if s == "**":
            # `**/` = zero or more whole segments; a final `**` = anything that is left.
            parts.append(".*" if last else "(?:[^/]*/)*")
            continue
        if "**" in s:
            raise ValueError("partial **")
        for c in s:
            parts.append("[^/]*" if c == "*" else "[^/]" if c == "?" else re.escape(c))
        if not last:
            parts.append("/")
    return re.compile("".join(parts), re.DOTALL)


def glob_list(v, what):
    if not isinstance(v, list) or not all(isinstance(x, str) for x in v):
        raise PlanError("GATES-CONFIG", f"{what} must be an array of strings")
    if len(v) > 256 or len(set(v)) != len(v):
        raise PlanError("GATES-CONFIG", f"{what}: too many or duplicate globs")
    out = []
    for g in v:
        try:
            out.append((g, glob_regex(g)))
        except ValueError as e:
            raise PlanError("GATES-CONFIG", f"{what}: bad glob {g!r} ({e})") from None
    return out


# --- config ----------------------------------------------------------------------------

def load_config(path):
    data = path.read_bytes()
    if len(data) > MAX_CONFIG_BYTES:
        raise PlanError("GATES-CONFIG", "too large")
    try:
        top = tomllib.loads(data.decode("utf-8"))
    except (UnicodeDecodeError, tomllib.TOMLDecodeError) as e:
        raise PlanError("GATES-CONFIG", f"malformed: {e}") from None
    if set(top) - {"gate", "isolation", "shared"}:
        raise PlanError("GATES-CONFIG", "unknown top-level key")
    entries = top.get("gate")
    if not isinstance(entries, list) or not entries or len(entries) > 256:
        raise PlanError("GATES-CONFIG", "need 1..256 [[gate]] entries")
    gates = {}
    for e in entries:
        if not isinstance(e, dict) or set(e) - {"id", "run", "globs", "always"}:
            raise PlanError("GATES-CONFIG", "bad gate table or unknown gate key")
        gid, run = e.get("id"), e.get("run")
        if not isinstance(gid, str) or not ID_RE.match(gid) or len(gid) > 64:
            raise PlanError("GATES-CONFIG", f"bad id {gid!r}")
        if not isinstance(run, str) or not run.strip() or len(run.encode()) > 4096 or any(
            ord(c) < 0x20 or 0x7F <= ord(c) <= 0x9F for c in run
        ):
            raise PlanError("GATES-CONFIG", "bad run")
        always = e.get("always", False)
        if not isinstance(always, bool):
            raise PlanError("GATES-CONFIG", "bad always")
        if "globs" not in e:
            raise PlanError("GATES-CONFIG", "missing globs")
        globs = glob_list(e["globs"], f"gate {gid} globs")
        if not globs and not always:
            raise PlanError("GATES-CONFIG", f"gate {gid} can never be selected")
        if gid in gates:
            raise PlanError("GATES-CONFIG", f"duplicate id {gid}")
        gates[gid] = {"run": run, "globs": globs, "always": always}

    def section(name, key, default):
        t = top.get(name)
        if t is None:
            return glob_list(default, key)
        if not isinstance(t, dict) or set(t) - {key}:
            raise PlanError("GATES-CONFIG", f"bad [{name}]")
        return glob_list(t.get(key, default), key)

    ci = section("isolation", "ci_globs", DEFAULT_CI)
    if not ci:
        raise PlanError("GATES-CONFIG", "empty ci_globs")
    tools = section("shared", "tool_globs", DEFAULT_TOOLS)
    return gates, ci, tools


# --- changed paths ---------------------------------------------------------------------

def clean_path(line):
    if len(line.encode()) > MAX_PATH_BYTES:
        raise ValueError("long")
    if line != line.strip() or any(ord(c) < 0x20 or 0x7F <= ord(c) <= 0x9F for c in line):
        raise ValueError("whitespace or control")
    if "\ufeff" in line:
        raise ValueError("byte-order mark")
    if line.startswith('"'):
        raise ValueError("quoted")
    p = line.replace("\\", "/")
    if p.startswith("./"):
        p = p[2:]
    if p.startswith("/") or re.match(r"[A-Za-z]:", p):
        raise ValueError("absolute")
    if any(s in ("", ".", "..") for s in p.split("/")):
        raise ValueError("segment")
    return p


def load_changed(path):
    text = path.read_bytes().decode("utf-8")
    lines = [ln[:-1] if ln.endswith("\r") else ln for ln in text.split("\n")]
    lines = [ln for ln in lines if ln]
    if len(lines) > MAX_CHANGED_PATHS:
        raise PlanError("GATES-INPUT", "too many")
    out = set()
    for ln in lines:
        try:
            out.add(clean_path(ln))
        except ValueError as e:
            raise PlanError("GATES-INPUT", str(e)) from None
    if not out:
        raise PlanError("GATES-INPUT", "empty")
    return sorted(out)


# --- planner ---------------------------------------------------------------------------

def first_match(globs, p):
    for g, rx in globs:
        if rx.fullmatch(p):
            return g
    return None


def plan_case(config, changed):
    try:
        gates, ci, tools = load_config(HERE / config)
        paths = load_changed(HERE / changed)
    except PlanError as e:
        return {"exit": "2", "error": e.code, "json": "", "text": ""}
    ci_paths = [p for p in paths if first_match(ci, p)]
    other = [p for p in paths if not first_match(ci, p)]
    if ci_paths and other:
        mixed = "".join(f"CI-MIXED {p}\n" for p in other)
        return {"exit": "1", "error": None, "json": mixed, "text": mixed}
    trigger = next((p for p in paths if p.split("/")[-1] == "ci-gates.toml" or first_match(tools, p)), None)
    chosen = []
    for gid in sorted(gates):
        g = gates[gid]
        if g["always"]:
            chosen.append((gid, "always"))
        elif trigger is not None:
            chosen.append((gid, f"every gate selected: {trigger} is a ci-gates.toml or a shared tool"))
        else:
            hit = next(((p, first_match(g["globs"], p)) for p in paths if first_match(g["globs"], p)), None)
            if hit:
                chosen.append((gid, f"{hit[0]} matches {hit[1]}"))
    js = json.dumps({"gates": [c[0] for c in chosen]}, separators=(",", ":")) + "\n"
    if chosen:
        text = f"ci-plan: {len(chosen)} gate(s)\n" + "".join(f"{gid}: {why}\n" for gid, why in chosen)
    else:
        text = "ci-plan: 0 gates: no changed path matches a gate glob and no gate is always\n"
    return {"exit": "0", "error": None, "json": js, "text": text}


CFG = "ci-gates.toml"
ALW = "ci-gates-always.toml"
CASES = [
    # (name, config, changed). AC-1..AC-6 first, then the other rules.
    ("ac1_kernel_change", CFG, "kernel-change.txt"),
    ("ac2_spec_change", CFG, "spec-change.txt"),
    ("ac3_mixed", CFG, "mixed.txt"),
    ("ac4_config_change", CFG, "config-change.txt"),
    ("ac5_ci_only", CFG, "ci-only.txt"),
    ("ac6_multi", CFG, "multi.txt"),
    ("ac6_multi_shuffled", CFG, "multi-shuffled.txt"),
    ("config_copy_change", CFG, "config-copy-change.txt"),
    ("tool_change", CFG, "tool-change.txt"),
    ("no_match", CFG, "no-match.txt"),
    ("near_miss", CFG, "near-miss.txt"),
    ("mixed_many", CFG, "mixed-many.txt"),
    ("normalise", CFG, "normalise.txt"),
    ("always_kernel", ALW, "kernel-change.txt"),
    ("always_no_match", ALW, "no-match.txt"),
    ("always_ci_only", ALW, "ci-only.txt"),
    ("always_mixed", ALW, "mixed.txt"),
    ("always_tool", ALW, "tool-change.txt"),
    ("always_check_tool", ALW, "check-tool-change.txt"),
    ("always_multi", ALW, "multi.txt"),
    ("bad_input_dotdot", CFG, "bad-dotdot.txt"),
    ("bad_input_absolute", CFG, "bad-absolute.txt"),
    ("bad_input_space", CFG, "bad-space.txt"),
    ("bad_input_quoted", CFG, "bad-quoted.txt"),
    ("bad_input_empty", CFG, "bad-empty.txt"),
    ("bad_input_bom", CFG, "bad-bom.txt"),
] + [
    (f"bad_config_{p.stem.replace('-', '_')}", f"bad-config/{p.name}", "kernel-change.txt")
    for p in sorted((HERE / "bad-config").glob("*.toml"))
]

GLOB_CASES = [
    ("**/Cargo.toml", "Cargo.toml"), ("**/Cargo.toml", "crates/a/Cargo.toml"),
    ("**/Cargo.toml", "crates/a/Cargo.toml.orig"), ("**/Cargo.toml", "xCargo.toml"),
    ("spec/**", "spec"), ("spec/**", "spec/a"), ("spec/**", "spec/a/b/c"), ("spec/**", "specs/a"),
    ("*.md", "README.md"), ("*.md", "docs/a.md"), ("*.md", ".md"),
    ("docs/*.md", "docs/a.md"), ("docs/*.md", "docs/x/a.md"),
    ("README.?d", "README.md"), ("README.?d", "README.d"), ("README.?d", "README./d"),
    ("a/**/b", "a/b"), ("a/**/b", "a/x/y/b"), ("a/**/b", "a/xb"), ("a/**/b", "ab"), ("a/**/b", "a/b/c"),
    ("**", "anything/at/all"), ("**", "x"),
    ("**/x/**", "x/y"), ("**/x/**", "a/x"), ("**/x/**", "a/x/b"), ("**/x/**", "ax/b"),
    ("*", "a"), ("*", "a/b"), ("?", "/"), ("?", "a"), ("?", "ab"),
    ("a*b*c", "abc"), ("a*b*c", "aXbYc"), ("a*b*c", "aXbY"), ("a*b*c", "a/b/c"), ("a*b*c", "abcbc"),
    ("*/*", "a/b"), ("*/*", "a"), ("*/*", "a/b/c"),
    (".github/workflows/**", ".github/workflows/ci.yml"), (".github/workflows/**", ".github/workflowsx/ci.yml"),
    ("ci/**", "ci-gates.toml"), ("ci/**", "ci/fixture-checks.txt"),
    ("café/*", "café/x"), ("caf?/x", "café/x"), ("caf?/x", "cafe/x"),
    ("**/**/z", "z"), ("**/**/z", "a/b/z"), ("**/**/z", "a/bz"),
    ("tools/check-*.py", "tools/check-docs.py"), ("tools/check-*.py", "tools/x/check-a.py"),
]

# Long, backtracking-heavy inputs, hand-labelled (Python's backtracking regex engine takes
# exponential time on them, which is exactly why the planner's matcher is an automaton).
# Neither path contains the final literal the glob ends with, so neither can match.
HAND_GLOB_CASES = [
    ("*a*a*a*a*a*a*a*a*b", "a" * 3000, False),
    ("**/a/**/a/**/b", "a/" * 1500 + "c", False),
    ("**/a/**/a/**/b", "a/" * 1500 + "b", True),
]

BAD_GLOBS = [
    "", "/spec/**", "spec/", "spec//x", "spec/../x", "./spec", "[ab]", "{a,b}", "!spec/**",
    "a**", "**a", "a/b**/c", "spec\\x", "tab\there", "a" * 257,
]


def build():
    cases = [{"name": n, "config": c, "changed": ch, **plan_case(c, ch)} for n, c, ch in CASES]
    globs = [{"glob": g, "path": p, "match": bool(glob_regex(g).fullmatch(p))} for g, p in GLOB_CASES]
    for g, p, m in HAND_GLOB_CASES:
        glob_regex(g)  # still a valid glob
        globs.append({"glob": g, "path": p, "match": m})
    bad = []
    for g in BAD_GLOBS:
        try:
            glob_regex(g)
        except ValueError:
            bad.append(g)
        else:
            raise SystemExit(f"oracle accepted a glob it should refuse: {g!r}")
    # Hand-labelled anchors from the spec's acceptance criteria, so the oracle cannot drift.
    by = {c["name"]: c for c in cases}
    assert by["ac1_kernel_change"]["json"] == '{"gates":["layers"]}\n'
    assert '"spec"' in by["ac2_spec_change"]["json"] and '"layers"' not in by["ac2_spec_change"]["json"]
    assert by["ac3_mixed"]["exit"] == "1" and by["ac3_mixed"]["json"] == "CI-MIXED src/lib.rs\n"
    assert by["ac4_config_change"]["json"] == '{"gates":["apps","layers","rules","spec"]}\n'
    assert by["ac5_ci_only"]["exit"] == "0"
    assert by["ac5_ci_only"]["json"] == '{"gates":["apps","layers","rules","spec"]}\n'
    assert by["bad_input_bom"]["error"] == "GATES-INPUT"
    assert by["ac6_multi"] == {**by["ac6_multi_shuffled"], "name": "ac6_multi", "changed": "multi.txt"}
    return {"cases": cases, "glob_cases": globs, "bad_globs": bad}


def main():
    text = json.dumps(build(), indent=1, ensure_ascii=True, sort_keys=True) + "\n"
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print("fixtures/gates/expected.json is stale; run python fixtures/gates/make_cases.py")
            return 1
        print("fixtures/gates/expected.json is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT.relative_to(HERE.parent.parent)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
