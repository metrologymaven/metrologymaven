#!/usr/bin/env python3
"""Drift check between the fid-pkg crate and the published Result Package spec
(TRUST.PKG.FIXTURE REQ-005, AC-4).

Fails, naming each item, when one of these exists in the code but not in
docs/result-package/v1.md, or the reverse:

- error codes: every `"pkg.<code>"` literal in crates/fid-pkg/src/error.rs and
  crates/fid-pkg/src/reader/error.rs, against the spec's `### `pkg.<code>`` headings;
- reason tags of `pkg.limit` and `pkg.malformed`: the LIMIT_REASONS / MALFORMED_REASONS arrays
  plus every `limit("<tag>"` / `malformed("<tag>"` use in crates/fid-pkg/src/reader/, and of
  `pkg.digest_mismatch`: every `ReadError::mismatch(_, "<tag>")` use, against the first column
  of the spec's `<!-- reasons pkg.<code> -->` tables;
- manifest and statement fields: every `("<field>", ...)` key the serialiser in
  crates/fid-pkg/src/manifest.rs writes, against the first column of the spec's
  `<!-- fields ... -->` tables;
- reserved member folders: `MEMBER_DIRS` in crates/fid-pkg/src/manifest.rs, against
  the folder prefixes in the spec's `<!-- members -->` table (excluding `signatures/`).

It then runs a self-test: it adds an error code to a copy of the code text and checks that the
comparison fails naming it (AC-4), and likewise for a reason, a field and a member dir.

Stdlib only. Run: python fixtures/pkg/check_spec_drift.py
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SPEC = ROOT / "docs/result-package/v1.md"
SRC = ROOT / "crates/fid-pkg/src"
ERROR_FILES = [SRC / "error.rs", SRC / "reader/error.rs"]
READER_FILES = sorted((SRC / "reader").glob("*.rs"))
MANIFEST_FILE = SRC / "manifest.rs"


def array(text, name):
    m = re.search(r"pub const " + name + r": \[&str; \d+\] = \[(.*?)\];", text, re.S)
    return set(re.findall(r'"([a-z0-9_]+)"', m.group(1))) if m else set()


def code_items(errors_text, reader_text, manifest_text):
    """{"codes": set, "reasons:<code>": set, "fields": set, "member_dirs": set} from the crate's sources."""
    items = {"codes": set(re.findall(r'"(pkg\.[a-z0-9_]+)"', errors_text))}
    items["reasons:pkg.limit"] = array(errors_text, "LIMIT_REASONS") | set(
        re.findall(r'\blimit\("([a-z0-9_]+)"', reader_text))
    items["reasons:pkg.malformed"] = array(errors_text, "MALFORMED_REASONS") | set(
        re.findall(r'\bmalformed\("([a-z0-9_]+)"', reader_text))
    items["reasons:pkg.digest_mismatch"] = set(
        re.findall(r'ReadError::mismatch\([^,()]+(?:\([^()]*\))?,\s*"([a-z0-9_]+)"\)', reader_text))
    items["fields"] = set(re.findall(r'\(\s*"([A-Za-z_][A-Za-z0-9_]*)",\s*(?:s\(|CValue::|obj\()', manifest_text))
    items["member_dirs"] = array(manifest_text, "MEMBER_DIRS")
    return items


def first_column(block):
    return set(re.findall(r"^\| `([^`]+)` \|", block, re.M))


def spec_items(spec_text):
    items = {
        "codes": set(re.findall(r"^### `(pkg\.[a-z0-9_]+)`\s*$", spec_text, re.M)),
        "fields": set(),
        "member_dirs": set(),
    }
    for m in re.finditer(r"<!-- reasons (pkg\.[a-z0-9_]+) -->(.*?)<!-- /reasons -->", spec_text, re.S):
        items["reasons:" + m.group(1)] = first_column(m.group(2))
    for m in re.finditer(r"<!-- fields [a-z]+ -->(.*?)<!-- /fields -->", spec_text, re.S):
        items["fields"] |= first_column(m.group(1))
    for m in re.finditer(r"<!-- members -->(.*?)<!-- /members -->", spec_text, re.S):
        items["member_dirs"] |= {
            p.split("/", 1)[0]
            for p in first_column(m.group(1))
            if "/" in p and not p.startswith("signatures/")
        }
    return items


def compare(code, spec):
    out = []
    for key in sorted(set(code) | set(spec)):
        c, s = code.get(key, set()), spec.get(key, set())
        for x in sorted(c - s):
            out.append(f"spec.drift {key}: `{x}` is in the code but not in docs/result-package/v1.md")
        for x in sorted(s - c):
            out.append(f"spec.drift {key}: `{x}` is in docs/result-package/v1.md but not in the code")
    return out


def self_test(errors_text, reader_text, manifest_text, spec):
    injected = errors_text + '\n            ReadError::Injected => "pkg.injected_for_self_test",\n'
    got = compare(code_items(injected, reader_text, manifest_text), spec)
    assert any("`pkg.injected_for_self_test` is in the code" in g for g in got), got
    got = compare(code_items(errors_text, reader_text + '\nmalformed("injected_reason", "x")', manifest_text), spec)
    assert any("reasons:pkg.malformed: `injected_reason`" in g for g in got), got
    got = compare(code_items(errors_text, reader_text, manifest_text + '\n("injected_field", s(&x)),'), spec)
    assert any("fields: `injected_field`" in g for g in got), got
    mutated = manifest_text.replace('MEMBER_DIRS: [&str; 6] = [', 'MEMBER_DIRS: [&str; 6] = ["injected_dir", ', 1)
    assert mutated != manifest_text, "MEMBER_DIRS signature not found in manifest.rs"
    got = compare(code_items(errors_text, reader_text, mutated), spec)
    assert any("member_dirs: `injected_dir`" in g for g in got), got


def main():
    errors_text = "\n".join(p.read_text(encoding="utf-8") for p in ERROR_FILES)
    reader_text = "\n".join(p.read_text(encoding="utf-8") for p in READER_FILES)
    manifest_text = MANIFEST_FILE.read_text(encoding="utf-8")
    code = code_items(errors_text, reader_text, manifest_text)
    spec = spec_items(SPEC.read_text(encoding="utf-8"))
    for key in ("codes", "reasons:pkg.limit", "reasons:pkg.malformed", "reasons:pkg.digest_mismatch", "fields", "member_dirs"):
        if not code.get(key):
            print(f"spec.drift {key}: nothing found in the code; the extraction patterns are stale")
            return 1
    problems = compare(code, spec)
    for p in problems:
        print(p)
    if problems:
        return 1
    self_test(errors_text, reader_text, manifest_text, spec)
    n = {k: len(v) for k, v in code.items()}
    print(f"spec and code agree: {n['codes']} codes, {n['reasons:pkg.limit']} limit, "
          f"{n['reasons:pkg.malformed']} malformed and {n['reasons:pkg.digest_mismatch']} digest reasons, "
          f"{n['fields']} fields, {n['member_dirs']} member dirs; self-test names an added code, reason, field and member dir")
    return 0


if __name__ == "__main__":
    sys.exit(main())
