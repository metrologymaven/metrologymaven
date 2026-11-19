"""Independent reference for the vendored-solver audit fixtures (KRN.SOLV.CONIC.VEND).

Stdlib only; shares no code with `cargo xtask conic-audit` (crates/fid-xtask/src/conic_audit.rs).
Every fixture tree here is synthetic and hand-written: a tiny stand-in "minisolv" crate, not
the real upstream solver. The script

* writes the hand-written upstream trees and patches,
* applies the patches with its own strict unified-diff applier (exact line positions, no
  fuzz, no offset) and checks the result against hand-written expected crate files,
* computes the tree digest of each `upstream/` (SHA-256 over the sorted lines
  `<sha256 hex of content>  <relative path>\\n`), written `sha256:<hex>`,
* writes the KNP-1 lint reports (schema 1, RFC 8785 canonical JSON) and the AUDIT.toml
  records with those digests.

Expected outcomes, hand-labelled from the leaf's rules (crates/fid-xtask/tests/conic_audit.rs):

  ok/                                   exit 0                         (AC-1)
  drift/      one byte changed in crate/src/lib.rs   AUDIT-PATCH-DRIFT crate/src/lib.rs (AC-2)
  digest/     one byte changed in upstream/README.md (and crate/ to match)  AUDIT-DIGEST (AC-3)
  update/     --previous ok/AUDIT.toml, record lists only 0001  AUDIT-PATCH-MISSING 0002 (AC-4)
  update/AUDIT-dropped.toml            0002 dropped with a justification, exit 0  (AC-4)
  lint/unwaived-pow.json   (on ok/)    AUDIT-LINT crate/src/x.rs KNP1-POW          (AC-5)
  lint/other-crate.json    (on ok/)    exit 0: the finding is in another crate
  lint/waived.json         (on ok/)    AUDIT-LINT: waived finding not in the record
  lint/waived.json + lint/AUDIT-waived.toml    exit 0
  licence/AUDIT-gpl.toml   (on ok/)    AUDIT-LICENCE
  schema/AUDIT-norule.toml (on ok/)    AUDIT-SCHEMA (patch 0002 names no rule)
  schema/AUDIT-scope.toml  (on ok/)    exit 0 (patch 0002 is a scope-reduction)

Run from the repository root:
    python fixtures/solv/vend/make_fixtures.py          # write
    python fixtures/solv/vend/make_fixtures.py --check  # compare only
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ----------------------------------------------------------------- upstream trees

CARGO_V1 = """[package]
name = "minisolv"
version = "0.1.0"
edition = "2021"
license = "MIT"
"""

CARGO_V2 = CARGO_V1.replace('version = "0.1.0"', 'version = "0.2.0"')

LICENCE = "Synthetic fixture licence notice for the conic-audit tests; the recorded licence is MIT.\n"

README_V1 = "minisolv: a hand-written stand-in for a vendored solver tree.\n"
README_V2 = "minisolv 0.2: a hand-written stand-in for a vendored solver tree.\n"

LIB = """//! Hand-written stand-in for a vendored solver (KRN.SOLV.CONIC.VEND fixture).
pub mod x;

pub fn norm2(v: &[f64]) -> f64 {
    let mut s = 0.0;
    for x in v {
        s += x.powi(2);
    }
    s
}
"""

X_HASH = """use std::collections::HashMap;

pub fn count(keys: &[u32]) -> HashMap<u32, usize> {
    let mut m = HashMap::new();
    for k in keys {
        *m.entry(*k).or_insert(0) += 1;
    }
    m
}
"""

X_BTREE = """use std::collections::BTreeMap;

pub fn count(keys: &[u32]) -> BTreeMap<u32, usize> {
    let mut m = BTreeMap::new();
    for k in keys {
        *m.entry(*k).or_insert(0) += 1;
    }
    m
}
"""

# Hand-written expected result of patch 0001 on LIB.
LIB_PATCHED = LIB.replace("x.powi(2)", "x * x")

PATCH_0001 = """From: conic-audit fixture
Subject: [PATCH 0001] Replace powi with a product (KNP1-POW)

--- a/src/lib.rs
+++ b/src/lib.rs
@@ -4,7 +4,7 @@
 pub fn norm2(v: &[f64]) -> f64 {
     let mut s = 0.0;
     for x in v {
-        s += x.powi(2);
+        s += x * x;
     }
     s
 }
"""

PATCH_0002 = """From: conic-audit fixture
Subject: [PATCH 0002] Use BTreeMap instead of HashMap (KNP1-HASH)

--- a/src/x.rs
+++ b/src/x.rs
@@ -1,7 +1,7 @@
-use std::collections::HashMap;
+use std::collections::BTreeMap;
<SPACE>
-pub fn count(keys: &[u32]) -> HashMap<u32, usize> {
-    let mut m = HashMap::new();
+pub fn count(keys: &[u32]) -> BTreeMap<u32, usize> {
+    let mut m = BTreeMap::new();
     for k in keys {
         *m.entry(*k).or_insert(0) += 1;
     }
""".replace(
    "<SPACE>", " "
)  # the blank context line is one space; spelled out so editors cannot strip it

UPSTREAM_V1 = {
    "Cargo.toml": CARGO_V1,
    "LICENSE": LICENCE,
    "README.md": README_V1,
    "src/lib.rs": LIB,
    "src/x.rs": X_HASH,
}

# The "new upstream commit": HashMap already fixed upstream, so patch 0002 is dropped.
UPSTREAM_V2 = {
    "Cargo.toml": CARGO_V2,
    "LICENSE": LICENCE,
    "README.md": README_V2,
    "src/lib.rs": LIB,
    "src/x.rs": X_BTREE,
}

PATCHES_V1 = {
    "patches/0001-no-powi.patch": PATCH_0001,
    "patches/0002-btreemap.patch": PATCH_0002,
}
PATCHES_V2 = {"patches/0001-no-powi.patch": PATCH_0001}

EXPECTED_CRATE_V1 = dict(UPSTREAM_V1, **{"src/lib.rs": LIB_PATCHED, "src/x.rs": X_BTREE})
EXPECTED_CRATE_V2 = dict(UPSTREAM_V2, **{"src/lib.rs": LIB_PATCHED})

COMMIT_V1 = "1111111111111111111111111111111111111111"
COMMIT_V2 = "2222222222222222222222222222222222222222"

# ----------------------------------------------------------------- strict patch applier


def apply_patch(files: dict, patch: str) -> dict:
    """Applies a unified diff with exact positions; raises ValueError on any mismatch."""
    files = dict(files)
    lines = patch.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    i = 0
    while i < len(lines):
        if not lines[i].startswith("--- "):
            i += 1
            continue
        old = lines[i][4:].split("\t")[0]
        new = lines[i + 1][4:].split("\t")[0]
        if not lines[i + 1].startswith("+++ "):
            raise ValueError("missing +++ line")
        i += 2
        if not (old.startswith("a/") and new.startswith("b/") and old[2:] == new[2:]):
            raise ValueError("this reference applier handles modifications only")
        path = old[2:]
        src = files[path].split("\n")
        assert src[-1] == "", "fixture files end with a newline"
        src = src[:-1]
        out, cursor = [], 0
        while i < len(lines) and lines[i].startswith("@@ "):
            head = lines[i].split(" ")
            o_start, o_len = (int(v) for v in head[1][1:].split(","))
            n_start, n_len = (int(v) for v in head[2][1:].split(","))
            i += 1
            out.extend(src[cursor : o_start - 1])
            pos, seen_old, seen_new = o_start - 1, 0, 0
            while seen_old < o_len or seen_new < n_len:
                tag, text = lines[i][0], lines[i][1:]
                if tag in " -":
                    if src[pos] != text:
                        raise ValueError(f"{path}: line {pos + 1} does not match")
                    pos += 1
                    seen_old += 1
                if tag in " +":
                    out.append(text)
                    seen_new += 1
                if tag not in " -+":
                    raise ValueError(f"bad hunk line {lines[i]!r}")
                i += 1
            cursor = pos
            assert n_start == o_start, "fixture hunks keep their position"
        out.extend(src[cursor:])
        files[path] = "\n".join(out) + "\n"
    return files


def tree_digest(files: dict) -> str:
    text = "".join(f"{hashlib.sha256(files[p].encode()).hexdigest()}  {p}\n" for p in sorted(files))
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def crate_from(upstream: dict, patches: dict, expected: dict) -> dict:
    files = dict(upstream)
    for name in sorted(patches):
        files = apply_patch(files, patches[name])
    if files != expected:
        raise SystemExit("reference applier disagrees with the hand-written crate files")
    return files


# ----------------------------------------------------------------- lint reports


def sha(text: str) -> str:
    return "sha256:" + hashlib.sha256(text.encode()).hexdigest()


def canonical(v) -> str:
    # RFC 8785 for this data: ASCII strings, small integers, keys sorted, no whitespace.
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def finding(crate: str, file: str, line: int, rule: str) -> dict:
    return {
        "rule": rule,
        "crate_name": crate,
        "crate_version": "0.1.0",
        "file": file,
        "line": line,
        "column": 14,
        "excerpt_digest": sha(f"{crate} {file} {line}"),
        "message": f"{rule} in a fixture",
    }


def report(findings: list, waived: list) -> str:
    by_rule = {}
    for f in findings:
        by_rule[f["rule"]] = by_rule.get(f["rule"], 0) + 1
    return canonical(
        {
            "schema": 1,
            "tool": "fid-knp1-lint",
            "tool_version": "0.0.1",
            "inputs": {"config": sha("config"), "lockfile": sha("lockfile"), "waivers": sha("waivers"), "denylist": sha("denylist")},
            "findings": findings,
            "waived": waived,
            "warnings": [],
            "summary": {"findings": len(findings), "waived": len(waived), "warnings": 0, "by_rule": by_rule},
        }
    )


CLEAN = report([], [])
UNWAIVED_POW = report([finding("minisolv", "src/x.rs", 6, "KNP1-POW")], [])
OTHER_CRATE = report([finding("othercrate", "src/x.rs", 6, "KNP1-POW")], [])
WAIVER = {
    "crate": "minisolv",
    "file": "src/lib.rs",
    "lines": [7, 7],
    "rule": "KNP1-POW",
    "justification": "fixture waiver",
    "reviewer": "fixture",
    "date": "2026-11-18",
    "lines_sha256": sha("        s += x * x;\n"),
}
WAIVED = report([], [{"finding": finding("minisolv", "src/lib.rs", 7, "KNP1-POW"), "waiver": WAIVER}])

# ----------------------------------------------------------------- audit records

RATIONALE = {
    "0001": "powi is outside the fid-num facade; a product is exact for squares",
    "0002": "HashMap iteration order is not fixed; BTreeMap is",
}


def audit(commit, tree, report_text, patches, rules=None, dropped=(), waivers=(), licence="MIT") -> str:
    rules = rules or {"0001": ["KNP1-POW"], "0002": ["KNP1-HASH"]}
    out = [
        "# Audit record (KRN.SOLV.CONIC.VEND). Synthetic fixture written by make_fixtures.py.",
        "schema = 1",
        f'lint_report_sha256 = "{sha(report_text)}"',
        'auditor = "conic-audit fixture"',
        'date = "2026-11-18"',
        "",
        "[upstream]",
        'url = "https://example.invalid/minisolv.git"',
        f'commit = "{commit}"',
        f'tree_sha256 = "{tree}"',
        f'licence = "{licence}"',
    ]
    for name in sorted(patches):
        pid = name.split("/")[1][:4]
        rl = ", ".join(f'"{r}"' for r in rules[pid])
        out += ["", "[[patches]]", f'id = "{pid}"', f'file = "{name}"', f"rules = [{rl}]", f'rationale = "{RATIONALE[pid]}"']
    for pid, why in dropped:
        out += ["", "[[dropped]]", f'id = "{pid}"', f'justification = "{why}"']
    for ref, why in waivers:
        out += ["", "[[waivers]]", f'waiver_ref = "{ref}"', f'justification = "{why}"']
    return "\n".join(out) + "\n"


def tree_files(prefix: str, files: dict) -> dict:
    return {f"{prefix}/{k}": v for k, v in files.items()}


def build() -> dict:
    out = {}
    crate_v1 = crate_from(UPSTREAM_V1, PATCHES_V1, EXPECTED_CRATE_V1)
    crate_v2 = crate_from(UPSTREAM_V2, PATCHES_V2, EXPECTED_CRATE_V2)
    tree_v1 = tree_digest(UPSTREAM_V1)
    ok_audit = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1)

    def case(name, upstream, crate, patches, record):
        out.update(tree_files(f"{name}/upstream", upstream))
        out.update(tree_files(f"{name}/crate", crate))
        out.update({f"{name}/{k}": v for k, v in patches.items()})
        out[f"{name}/AUDIT.toml"] = record
        out[f"{name}/knp1-findings.json"] = CLEAN

    # AC-1
    case("ok", UPSTREAM_V1, crate_v1, PATCHES_V1, ok_audit)
    # AC-2: one byte changed in crate/src/lib.rs ("written" -> "writtem").
    drifted = dict(crate_v1, **{"src/lib.rs": crate_v1["src/lib.rs"].replace("Hand-written", "Hand-writtem", 1)})
    case("drift", UPSTREAM_V1, drifted, PATCHES_V1, ok_audit)
    # AC-3: one byte changed in upstream/README.md; crate/ changed the same way, so only the
    # recorded digest is stale.
    readme = README_V1.replace("minisolv:", "minisolv;", 1)
    case("digest", dict(UPSTREAM_V1, **{"README.md": readme}), dict(crate_v1, **{"README.md": readme}), PATCHES_V1, ok_audit)
    # AC-4: new upstream commit; only patch 0001 carried forward.
    tree_v2 = tree_digest(UPSTREAM_V2)
    case("update", UPSTREAM_V2, crate_v2, PATCHES_V2, audit(COMMIT_V2, tree_v2, CLEAN, PATCHES_V2))
    out["update/AUDIT-dropped.toml"] = audit(COMMIT_V2, tree_v2, CLEAN, PATCHES_V2, dropped=[("0002", "fixed upstream")])
    # AC-5 and the other lint cases, run against ok/ with --report (and --audit).
    out["lint/unwaived-pow.json"] = UNWAIVED_POW
    out["lint/other-crate.json"] = OTHER_CRATE
    out["lint/waived.json"] = WAIVED
    out["lint/AUDIT-waived.toml"] = audit(
        COMMIT_V1, tree_v1, WAIVED, PATCHES_V1, waivers=[("src/lib.rs:7-7:KNP1-POW", "fixture: waiver kept on purpose")]
    )
    # REQ-003 and REQ-006, run against ok/ with --audit.
    out["licence/AUDIT-gpl.toml"] = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1, licence="GPL-3.0-only")
    out["licence/AUDIT-or.toml"] = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1, licence="GPL-3.0-only OR MIT")
    out["schema/AUDIT-norule.toml"] = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1, rules={"0001": ["KNP1-POW"], "0002": []})
    out["schema/AUDIT-scope.toml"] = audit(
        COMMIT_V1, tree_v1, CLEAN, PATCHES_V1, rules={"0001": ["KNP1-POW"], "0002": ["scope-reduction"]}
    )
    return out


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build()
    bad = 0
    for rel in sorted(files):
        path = HERE / rel
        data = files[rel].encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}")
                bad = 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    expected = set(files) | {"make_fixtures.py"}
    for p in sorted(HERE.rglob("*")):
        rel = p.relative_to(HERE).as_posix()
        if p.is_file() and rel not in expected and "__pycache__" not in rel:
            print(f"unexpected file: {p}")
            bad = 1
    if check and not bad:
        print(f"solv/vend fixtures: ok ({len(files)} files)")
    return bad


if __name__ == "__main__":
    sys.exit(main())
