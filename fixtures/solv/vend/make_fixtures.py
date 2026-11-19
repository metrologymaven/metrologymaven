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
  createdelete/  patch 0003 deletes README.md and creates src/extra.rs   exit 0

  Every tree holds data/crlf.txt with CRLF line ends (the directory is -text), so the recorded
  digests prove the check hashes bytes exactly as committed.

  Lint cases run on ok/ with --report lint/<name>.json --audit lint/AUDIT-<name>.toml (each record
  carries the SHA-256 of its report):
  unwaived-pow             AUDIT-LINT crate/src/x.rs KNP1-POW          (AC-5)
  other-crate              exit 0: the finding is in another crate
  waived                   AUDIT-LINT: waived finding not in the record
  waived-listed            (waived.json) exit 0
  not-linted               AUDIT-LINT: the report does not name minisolv 0.1.0 as linted
  wrong-version            AUDIT-LINT: the report linted minisolv 0.0.9
  waiver-crate             AUDIT-LINT: the waiver names another crate
  ok/AUDIT.toml with lint/other-crate.json   AUDIT-LINT: lint_report_sha256 does not match

  licence/AUDIT-gpl.toml       (on ok/)  AUDIT-LICENCE
  licence/AUDIT-or.toml        (on ok/)  exit 0 (GPL-3.0-only OR MIT)
  licence/AUDIT-mit-with.toml  (on ok/)  AUDIT-LICENCE (MIT WITH LLVM-exception is not a deny.toml pair)
  licence/AUDIT-apache-with.toml (on ok/) exit 0 (Apache-2.0 WITH LLVM-exception is)
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

PATCH_0003 = """From: conic-audit fixture
Subject: [PATCH 0003] Drop the README, add a constant (scope-reduction)

--- a/README.md
+++ /dev/null
@@ -1 +0,0 @@
-minisolv: a hand-written stand-in for a vendored solver tree.
--- /dev/null
+++ b/src/extra.rs
@@ -0,0 +1,2 @@
+//! Added by patch 0003 (fixture).
+pub const EXTRA: u32 = 3;
"""

EXTRA = "//! Added by patch 0003 (fixture).\npub const EXTRA: u32 = 3;\n"

# CRLF line ends on purpose: the digest must hash the committed bytes.
CRLF = "row 1\r\nrow 2\r\n"

UPSTREAM_V1 = {
    "Cargo.toml": CARGO_V1,
    "LICENSE": LICENCE,
    "README.md": README_V1,
    "data/crlf.txt": CRLF,
    "src/lib.rs": LIB,
    "src/x.rs": X_HASH,
}

# The "new upstream commit": HashMap already fixed upstream, so patch 0002 is dropped.
UPSTREAM_V2 = {
    "Cargo.toml": CARGO_V2,
    "LICENSE": LICENCE,
    "README.md": README_V2,
    "data/crlf.txt": CRLF,
    "src/lib.rs": LIB,
    "src/x.rs": X_BTREE,
}

PATCHES_V1 = {
    "patches/0001-no-powi.patch": PATCH_0001,
    "patches/0002-btreemap.patch": PATCH_0002,
}
PATCHES_V2 = {"patches/0001-no-powi.patch": PATCH_0001}
PATCHES_V3 = dict(PATCHES_V1, **{"patches/0003-add-remove.patch": PATCH_0003})

EXPECTED_CRATE_V1 = dict(UPSTREAM_V1, **{"src/lib.rs": LIB_PATCHED, "src/x.rs": X_BTREE})
EXPECTED_CRATE_V2 = dict(UPSTREAM_V2, **{"src/lib.rs": LIB_PATCHED})
EXPECTED_CRATE_V3 = {k: v for k, v in EXPECTED_CRATE_V1.items() if k != "README.md"}
EXPECTED_CRATE_V3["src/extra.rs"] = EXTRA

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
        if old == "/dev/null" and new.startswith("b/"):
            path, kind = new[2:], "create"
            if path in files:
                raise ValueError(f"{path} already exists")
            src = []
        elif new == "/dev/null" and old.startswith("a/"):
            path, kind = old[2:], "delete"
        elif old.startswith("a/") and new.startswith("b/") and old[2:] == new[2:]:
            path, kind = old[2:], "modify"
        else:
            raise ValueError("unsupported header pair")
        if kind != "create":
            src = files[path].split("\n")
            assert src[-1] == "", "fixture files end with a newline"
            src = src[:-1]
        out, cursor = [], 0
        while i < len(lines) and lines[i].startswith("@@ "):
            head = lines[i].split(" ")

            def rng(t):
                a, _, b = t[1:].partition(",")
                return int(a), int(b) if b else 1

            o_start, o_len = rng(head[1])
            n_start, n_len = rng(head[2])
            i += 1
            first = o_start if o_len == 0 else o_start - 1
            out.extend(src[cursor:first])
            pos, seen_old, seen_new = first, 0, 0
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
            assert kind != "modify" or n_start == o_start, "fixture hunks keep their position"
        out.extend(src[cursor:])
        if kind == "delete":
            if out:
                raise ValueError(f"deleting {path} leaves lines")
            del files[path]
        else:
            files[path] = "\n".join(out) + "\n"
    return files


def tree_digest(files: dict) -> str:
    # Hashes the exact bytes (UTF-8, CR kept): "\r\n" in a file stays two bytes.
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


def finding(crate: str, file: str, line: int, rule: str, version: str = "0.1.0") -> dict:
    return {
        "rule": rule,
        "crate_name": crate,
        "crate_version": version,
        "file": file,
        "line": line,
        "column": 14,
        "excerpt_digest": sha(f"{crate} {file} {line}"),
        "message": f"{rule} in a fixture",
    }


def report(findings: list, waived: list, linted=(("minisolv", "0.1.0"),)) -> str:
    by_rule = {}
    for f in findings:
        by_rule[f["rule"]] = by_rule.get(f["rule"], 0) + 1
    return canonical(
        {
            "schema": 1,
            "tool": "fid-knp1-lint",
            "tool_version": "0.0.1",
            "inputs": {"config": sha("config"), "lockfile": sha("lockfile"), "waivers": sha("waivers"), "denylist": sha("denylist")},
            "linted": [{"crate_name": n, "crate_version": v} for n, v in sorted(linted)],
            "findings": findings,
            "waived": waived,
            "warnings": [],
            "summary": {"findings": len(findings), "waived": len(waived), "warnings": 0, "by_rule": by_rule},
        }
    )


CLEAN = report([], [])
CLEAN_V2 = report([], [], linted=[("minisolv", "0.2.0")])


def waiver(crate: str) -> dict:
    return {
        "crate": crate,
        "file": "src/lib.rs",
        "lines": [7, 7],
        "rule": "KNP1-POW",
        "justification": "fixture waiver",
        "reviewer": "fixture",
        "date": "2026-11-18",
        "lines_sha256": sha("        s += x * x;\n"),
    }


WAIVED_FINDING = finding("minisolv", "src/lib.rs", 7, "KNP1-POW")
BOTH = [("minisolv", "0.1.0"), ("othercrate", "0.1.0")]
WAIVER_REF = [("src/lib.rs:7-7:KNP1-POW", "fixture: waiver kept on purpose")]

# name: (report, waivers listed in the record)
LINT_CASES = {
    "unwaived-pow": (report([finding("minisolv", "src/x.rs", 6, "KNP1-POW")], []), ()),
    "other-crate": (report([finding("othercrate", "src/x.rs", 6, "KNP1-POW")], [], linted=BOTH), ()),
    "waived": (report([], [{"finding": WAIVED_FINDING, "waiver": waiver("minisolv")}]), ()),
    "waived-listed": (report([], [{"finding": WAIVED_FINDING, "waiver": waiver("minisolv")}]), WAIVER_REF),
    "not-linted": (report([], [], linted=[("othercrate", "0.1.0")]), ()),
    "wrong-version": (report([], [], linted=[("minisolv", "0.0.9")]), ()),
    "waiver-crate": (report([], [{"finding": WAIVED_FINDING, "waiver": waiver("othercrate")}]), WAIVER_REF),
}

# ----------------------------------------------------------------- audit records

RATIONALE = {
    "0001": "powi is outside the fid-num facade; a product is exact for squares",
    "0002": "HashMap iteration order is not fixed; BTreeMap is",
    "0003": "the README is not built; the constant is a fixture",
}


def audit(commit, tree, report_text, patches, rules=None, dropped=(), waivers=(), licence="MIT") -> str:
    rules = rules or {"0001": ["KNP1-POW"], "0002": ["KNP1-HASH"], "0003": ["scope-reduction"]}
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
    crate_v3 = crate_from(UPSTREAM_V1, PATCHES_V3, EXPECTED_CRATE_V3)
    tree_v1 = tree_digest(UPSTREAM_V1)
    ok_audit = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1)

    def case(name, upstream, crate, patches, record, rep=CLEAN):
        out.update(tree_files(f"{name}/upstream", upstream))
        out.update(tree_files(f"{name}/crate", crate))
        out.update({f"{name}/{k}": v for k, v in patches.items()})
        out[f"{name}/AUDIT.toml"] = record
        out[f"{name}/knp1-findings.json"] = rep

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
    case("update", UPSTREAM_V2, crate_v2, PATCHES_V2, audit(COMMIT_V2, tree_v2, CLEAN_V2, PATCHES_V2), CLEAN_V2)
    out["update/AUDIT-dropped.toml"] = audit(COMMIT_V2, tree_v2, CLEAN_V2, PATCHES_V2, dropped=[("0002", "fixed upstream")])
    # File creation and deletion through the file system.
    case("createdelete", UPSTREAM_V1, crate_v3, PATCHES_V3, audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V3))
    # AC-5 and the other lint cases, run against ok/ with --report and --audit.
    for name, (rep, waivers) in LINT_CASES.items():
        if name != "waived-listed":
            out[f"lint/{name}.json"] = rep
        out[f"lint/AUDIT-{name}.toml"] = audit(COMMIT_V1, tree_v1, rep, PATCHES_V1, waivers=waivers)
    # REQ-003 and REQ-006, run against ok/ with --audit.
    for name, lic in [
        ("gpl", "GPL-3.0-only"),
        ("or", "GPL-3.0-only OR MIT"),
        ("mit-with", "MIT WITH LLVM-exception"),
        ("apache-with", "Apache-2.0 WITH LLVM-exception"),
    ]:
        out[f"licence/AUDIT-{name}.toml"] = audit(COMMIT_V1, tree_v1, CLEAN, PATCHES_V1, licence=lic)
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
