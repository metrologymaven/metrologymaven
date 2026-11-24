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


def _extract_mismatch_reasons(reader_text):
    reasons = set()
    needle = "ReadError::mismatch("
    idx = 0
    n = len(reader_text)
    while True:
        pos = reader_text.find(needle, idx)
        if pos == -1:
            break
        start = pos + len(needle)
        depth = 1
        i = start
        top_comma = None
        in_str = False
        while i < n and depth > 0:
            ch = reader_text[i]
            if in_str:
                if ch == "\\":
                    i += 2
                    continue
                if ch == '"':
                    in_str = False
            else:
                if ch == '"':
                    in_str = True
                elif ch == "(":
                    depth += 1
                elif ch == ")":
                    depth -= 1
                    if depth == 0:
                        break
                elif ch == "," and depth == 1 and top_comma is None:
                    top_comma = i
            i += 1
        if top_comma is not None and depth == 0:
            arg2 = reader_text[top_comma + 1 : i].strip()
            m = re.match(r'^"([a-z0-9_]+)"$', arg2)
            if m:
                reasons.add(m.group(1))
        idx = start
    return reasons


def _impl_to_value_body(manifest_text, struct_name):
    m = re.search(
        r"impl\s+" + re.escape(struct_name) + r"\b.*?pub fn to_value\(&self\)\s*->\s*CValue\s*\{(.*?)\n    \}",
        manifest_text,
        re.S,
    )
    return m.group(1) if m else ""


def _extract_obj_fields(fn_body):
    return set(re.findall(r'\(\s*"([A-Za-z_][A-Za-z0-9_]*)"\s*,', fn_body))


def code_items(errors_text, reader_text, manifest_text):
    """{"codes": set, "reasons:<code>": set, "fields:manifest": set, "fields:statement": set, "member_dirs": set} from sources."""
    items = {"codes": set(re.findall(r'"(pkg\.[a-z0-9_]+)"', errors_text))}
    items["reasons:pkg.limit"] = array(errors_text, "LIMIT_REASONS") | set(
        re.findall(r'\blimit\(\s*"([a-z0-9_]+)"', reader_text))
    items["reasons:pkg.malformed"] = array(errors_text, "MALFORMED_REASONS") | set(
        re.findall(r'\bmalformed\(\s*"([a-z0-9_]+)"', reader_text))
    items["reasons:pkg.digest_mismatch"] = _extract_mismatch_reasons(reader_text)
    items["fields:manifest"] = _extract_obj_fields(_impl_to_value_body(manifest_text, "Manifest"))
    items["fields:statement"] = _extract_obj_fields(_impl_to_value_body(manifest_text, "Statement"))
    items["member_dirs"] = array(manifest_text, "MEMBER_DIRS")
    return items


def first_column(block):
    return set(re.findall(r"^\| `([^`]+)` \|", block, re.M))


def spec_items(spec_text):
    items = {
        "codes": set(re.findall(r"^### `(pkg\.[a-z0-9_]+)`\s*$", spec_text, re.M)),
        "fields:manifest": set(),
        "fields:statement": set(),
        "member_dirs": set(),
    }
    for m in re.finditer(r"<!-- reasons (pkg\.[a-z0-9_]+) -->(.*?)<!-- /reasons -->", spec_text, re.S):
        items["reasons:" + m.group(1)] = first_column(m.group(2))
    for m in re.finditer(r"<!-- fields ([a-z]+) -->(.*?)<!-- /fields -->", spec_text, re.S):
        kind, body = m.group(1), m.group(2)
        cols = first_column(body)
        if kind == "statement" and re.search(r"^\|\s*`digest`\s*\|[^|\n]*\|[^\n]*`sha256`", body, re.M):
            cols.add("sha256")
        items["fields:" + kind] = cols
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


def self_test(errors_text, reader_text, manifest_text, spec_text, spec):
    # 1. code -> spec drift across every category (including multiline, nested-paren mismatch, variable field, dynamic MEMBER_DIRS)
    injected = errors_text + '\n            ReadError::Injected => "pkg.injected_for_self_test",\n'
    got = compare(code_items(injected, reader_text, manifest_text), spec)
    assert any("`pkg.injected_for_self_test` is in the code" in g for g in got), got

    got = compare(code_items(errors_text, reader_text + '\nmalformed(\n    "injected_reason",\n    "x",\n)', manifest_text), spec)
    assert any("reasons:pkg.malformed: `injected_reason`" in g for g in got), got

    got = compare(
        code_items(errors_text, reader_text + '\nReadError::mismatch(foo.bar().as_str(),\n    "injected_mismatch")', manifest_text),
        spec,
    )
    assert any("reasons:pkg.digest_mismatch: `injected_mismatch`" in g for g in got), got

    mut_manifest = manifest_text.replace('("members", CValue::Arr(members)),', '("members", CValue::Arr(members)), ("injected_field", extra_var),', 1)
    assert mut_manifest != manifest_text
    got = compare(code_items(errors_text, reader_text, mut_manifest), spec)
    assert any("fields:manifest: `injected_field`" in g for g in got), got

    mutated = re.sub(r'(MEMBER_DIRS:\s*\[&str;\s*\d+\]\s*=\s*\[)', r'\1"injected_dir", ', manifest_text, count=1)
    assert mutated != manifest_text, "MEMBER_DIRS signature not found in manifest.rs"
    got = compare(code_items(errors_text, reader_text, mutated), spec)
    assert any("member_dirs: `injected_dir`" in g for g in got), got

    # 2. spec -> code drift and manifest/statement table isolation
    code = code_items(errors_text, reader_text, manifest_text)
    spec_added = spec_items(spec_text + "\n### `pkg.spec_only_code`\n")
    got = compare(code, spec_added)
    assert any("codes: `pkg.spec_only_code` is in docs/result-package/v1.md" in g for g in got), got

    swapped_spec = spec_text.replace("<!-- fields manifest -->\n| Field | Type | Meaning |\n|---|---|---|",
                                     "<!-- fields manifest -->\n| Field | Type | Meaning |\n|---|---|---|\n| `swapped_stmt_field` | string | test |", 1)
    assert swapped_spec != spec_text
    got = compare(code, spec_items(swapped_spec))
    assert any("fields:manifest: `swapped_stmt_field` is in docs/result-package/v1.md" in g for g in got), got


def main():
    errors_text = "\n".join(p.read_text(encoding="utf-8") for p in ERROR_FILES)
    reader_text = "\n".join(p.read_text(encoding="utf-8") for p in READER_FILES)
    manifest_text = MANIFEST_FILE.read_text(encoding="utf-8")
    spec_text = SPEC.read_text(encoding="utf-8")
    code = code_items(errors_text, reader_text, manifest_text)
    spec = spec_items(spec_text)
    for key in ("codes", "reasons:pkg.limit", "reasons:pkg.malformed", "reasons:pkg.digest_mismatch",
                "fields:manifest", "fields:statement", "member_dirs"):
        if not code.get(key):
            print(f"spec.drift {key}: nothing found in the code; the extraction patterns are stale")
            return 1
    problems = compare(code, spec)
    for p in problems:
        print(p)
    if problems:
        return 1
    self_test(errors_text, reader_text, manifest_text, spec_text, spec)
    n = {k: len(v) for k, v in code.items()}
    total_fields = n["fields:manifest"] + n["fields:statement"]
    print(f"spec and code agree: {n['codes']} codes, {n['reasons:pkg.limit']} limit, "
          f"{n['reasons:pkg.malformed']} malformed and {n['reasons:pkg.digest_mismatch']} digest reasons, "
          f"{total_fields} fields, {n['member_dirs']} member dirs; self-test names an added code, reason, field and member dir")
    return 0


if __name__ == "__main__":
    sys.exit(main())
