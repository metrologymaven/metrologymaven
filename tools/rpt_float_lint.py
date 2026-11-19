#!/usr/bin/env python3
"""RPT float rule, second layer (spec node RPT.MODEL.FMT, REQ-RPT.MODEL.FMT-002, AC-7).

RPT turns a value of record into text in one place only: module `fmt` of crate
`fid-report`, which calls the kernel formatters (`fid_num::fmt::format`) and holds no
float-to-decimal code of its own. The rule is enforced in two layers:

1. Types (first layer, compiler-checked). Every `crates/fid-report*` crate has a
   `clippy.toml` with `disallowed-types = [f32, f64]`, so CI's
   `cargo clippy --workspace --all-targets -- -D warnings` fails wherever a float type is
   named. Only `fid_report::fmt` and the listed boundary module `fid_report::json_num`
   carry `#![allow(clippy::disallowed_types)]`; outside them values travel as
   `fmt::SiValue` (no `Display`, `Debug` shows bits) or kernel types.
2. This scan (token level, stdlib only). Over every `*.rs` file under
   `crates/fid-report*/src/` it reports:

   - RPT-FLOAT-TYPE / RPT-FLOAT-LITERAL: the identifier `f32` or `f64`, or a float
     literal (`1.5`, `1e3`, `2f64`), outside the allowed files (ALLOWED_FLOAT_FILES), so a
     float cannot be created from a literal either, which clippy does not see;
   - RPT-LINT-ALLOW: an `allow`/`expect`/`warn` attribute naming `warnings`, `clippy::all`,
     `clippy::style`, `clippy::disallowed_types` or `clippy::disallowed_methods` outside
     the allowed files (it would switch layer 1 off);
   - RPT-FLOAT-SPEC: a formatting macro's format string with a precision (`{:.3}`,
     `{x:>8.2}`, `{:.*}`) or an exponent type (`{:e}`, `{:E}`), anywhere, `fmt` included;
   - RPT-FLOAT-ARG: a float literal or `f32`/`f64` among a formatting macro's arguments;
   - RPT-FLOAT-TOSTRING: `.to_string()` on a float literal or on a group mentioning one or
     `f32`/`f64`, or `f32::to_string` / `f64::to_string`;
   - RPT-FLOAT-TRAIT: `LowerExp` or `UpperExp`;
   - RPT-FLOAT-CRATE: a path through a float-printing crate (`ryu`, `dtoa`, ...);
   - RPT-LEX: a file the lexer cannot read (fail closed);
   - RPT-CONFIG: a `fid-report*` crate without a `clippy.toml` that disallows both `f32`
     and `f64`.

   Comments and string contents are not code. Integration tests are not shipped and not
   scanned (they may name floats, with `#![allow(clippy::disallowed_types)]`).

`--clippy-selftest` proves layer 1 in CI: it copies the fixture crate
`fixtures/rpt/model/fmt/lint/bad` (whose `src/lib.rs` holds the reviewed bypass cases)
with `crates/fid-report/clippy.toml` into a temporary directory, runs
`cargo clippy -- -D clippy::disallowed_types` on it, and requires a `disallowed_types`
error on every line annotated `clippy` in the fixture. It fails if clippy is missing.

Known limit of both layers: a float that reaches code outside `fmt` purely by inference
from another crate's API, with no float type named and no literal (for example a binding
`CValue::Num(f)` or `stmt.u_std()` passed straight to `format!`), is not seen. That case
is recorded as an open question of RPT.MODEL.FMT.

Exit status: 0 when there is no finding, 1 when there is at least one (each printed as
`path:line:col: RULE message`), 2 on a usage error.
"""

from __future__ import annotations

import argparse
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CRATE_GLOB = "fid-report*"
# Files (repository-relative, `/`-separated prefixes) where a float may be named.
ALLOWED_FLOAT_FILES = ("crates/fid-report/src/fmt/", "crates/fid-report/src/json_num.rs")
CLIPPY_FIXTURE = "fixtures/rpt/model/fmt/lint/bad"
LINT_SWITCHES = frozenset(["warnings", "all", "style", "disallowed_types", "disallowed_methods"])

FORMAT_MACROS = frozenset(
    """format format_args write writeln print println eprint eprintln panic assert assert_eq
    assert_ne debug_assert debug_assert_eq debug_assert_ne unreachable todo unimplemented
    concat stringify trace debug info warn error log""".split()
)
ASSERT_MACROS = frozenset(
    "assert assert_eq assert_ne debug_assert debug_assert_eq debug_assert_ne".split()
)
FLOAT_TRAITS =frozenset(["LowerExp", "UpperExp"])
FLOAT_CRATES = frozenset(["ryu", "dtoa", "lexical", "lexical_core", "float_pretty_print", "pretty_dtoa"])
FLOAT_TYPES = frozenset(["f32", "f64"])

IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
# A number literal starting at a digit (Rust lexer shape; suffixes included).
NUM_RE = re.compile(
    r"0[xob][0-9a-fA-F_]+(?:[iu](?:8|16|32|64|128|size))?"  # hex / octal / binary integers
    r"|[0-9][0-9_]*(?:\.[0-9][0-9_]*)?(?:[eE][+-]?[0-9_]*[0-9][0-9_]*)?(?:_?[A-Za-z][A-Za-z0-9_]*)?"
    r"|[0-9][0-9_]*\.(?![.A-Za-z_])"
)


@dataclass
class Tok:
    kind: str  # ident, num, float, str, char, life, punct, open, close
    text: str
    line: int
    col: int


@dataclass
class Finding:
    path: str
    line: int
    col: int
    rule: str
    message: str

    def __str__(self) -> str:
        return f"{self.path}:{self.line}:{self.col}: {self.rule} {self.message}"


class LexError(Exception):
    def __init__(self, line: int, col: int, what: str):
        super().__init__(what)
        self.line = line
        self.col = col
        self.what = what


def _is_float_literal(text: str) -> bool:
    if text[:2] in ("0x", "0o", "0b"):
        return False
    body = text.replace("_", "")
    if body.endswith(("f32", "f64")):
        return True
    if any(body.endswith(s) for s in ("i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize")):
        return False
    return "." in body or "e" in body or "E" in body


def lex(src: str) -> list[Tok]:
    """Tokenizes Rust source. Comments are dropped; string literal tokens keep their raw
    contents (between the quotes) in `text` with kind `str`."""
    toks: list[Tok] = []
    i, n = 0, len(src)
    line, col0 = 1, 0  # col0: index of the first character of the current line

    def pos(k: int) -> tuple[int, int]:
        return line, k - col0 + 1

    def advance_lines(a: int, b: int) -> None:
        nonlocal line, col0
        for k in range(a, b):
            if src[k] == "\n":
                line += 1
                col0 = k + 1

    while i < n:
        c = src[i]
        if c == "\n":
            line += 1
            col0 = i + 1
            i += 1
            continue
        if c.isspace():
            i += 1
            continue
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
            continue
        if src.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if src.startswith("/*", j):
                    depth += 1
                    j += 2
                elif src.startswith("*/", j):
                    depth -= 1
                    j += 2
                else:
                    j += 1
            if depth:
                raise LexError(*pos(i), "unterminated block comment")
            advance_lines(i, j)
            i = j
            continue
        # Raw strings: r"..", r#".."#, br"..", cr"..".
        m = re.compile(r"(?:b|c)?r(#*)\"").match(src, i)
        if m:
            hashes = m.group(1)
            end = '"' + hashes
            j = src.find(end, m.end())
            if j < 0:
                raise LexError(*pos(i), "unterminated raw string")
            ln, cl = pos(i)
            toks.append(Tok("str", src[m.end():j], ln, cl))
            advance_lines(i, j + len(end))
            i = j + len(end)
            continue
        # Plain strings: "..", b"..", c"..".
        m = re.compile(r"(?:b|c)?\"").match(src, i)
        if m:
            j = m.end()
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            if j >= n:
                raise LexError(*pos(i), "unterminated string")
            ln, cl = pos(i)
            toks.append(Tok("str", src[m.end():j], ln, cl))
            advance_lines(i, j + 1)
            i = j + 1
            continue
        # Char literals and lifetimes.
        m = re.compile(r"b?'(?:\\(?:x[0-9a-fA-F]{2}|u\{[0-9a-fA-F_]{1,8}\}|.)|[^\\'\n])'").match(src, i)
        if m:
            ln, cl = pos(i)
            toks.append(Tok("char", m.group(0), ln, cl))
            i = m.end()
            continue
        if c == "'":
            m = IDENT_RE.match(src, i + 1)
            if not m:
                raise LexError(*pos(i), "stray quote")
            ln, cl = pos(i)
            toks.append(Tok("life", src[i:m.end()], ln, cl))
            i = m.end()
            continue
        if c.isdigit():
            prev = toks[-1] if toks else None
            if prev is not None and prev.kind == "punct" and prev.text == ".":
                # Tuple index (`t.0`, `t.0.1`): integers only.
                m = re.compile(r"[0-9]+").match(src, i)
                assert m is not None
                ln, cl = pos(i)
                toks.append(Tok("num", m.group(0), ln, cl))
                i = m.end()
                continue
            m = NUM_RE.match(src, i)
            if not m:
                raise LexError(*pos(i), "bad number")
            text = m.group(0)
            ln, cl = pos(i)
            toks.append(Tok("float" if _is_float_literal(text) else "num", text, ln, cl))
            i = m.end()
            continue
        m = re.compile(r"r#[A-Za-z_][A-Za-z0-9_]*").match(src, i)
        if m:
            ln, cl = pos(i)
            toks.append(Tok("ident", m.group(0)[2:], ln, cl))
            i = m.end()
            continue
        m = IDENT_RE.match(src, i)
        if m:
            ln, cl = pos(i)
            toks.append(Tok("ident", m.group(0), ln, cl))
            i = m.end()
            continue
        if not c.isascii():
            raise LexError(*pos(i), f"stray character U+{ord(c):04X}")
        ln, cl = pos(i)
        if c in "([{":
            toks.append(Tok("open", c, ln, cl))
        elif c in ")]}":
            toks.append(Tok("close", c, ln, cl))
        elif src.startswith("::", i):
            toks.append(Tok("punct", "::", ln, cl))
            i += 2
            continue
        else:
            toks.append(Tok("punct", c, ln, cl))
        i += 1
    return toks


def match_close(toks: list[Tok], k: int) -> int:
    """Index of the token closing the group opened at `k` (or len(toks) if unbalanced)."""
    depth = 0
    for j in range(k, len(toks)):
        if toks[j].kind == "open":
            depth += 1
        elif toks[j].kind == "close":
            depth -= 1
            if depth == 0:
                return j
    return len(toks)


def match_open(toks: list[Tok], k: int) -> int:
    depth = 0
    for j in range(k, -1, -1):
        if toks[j].kind == "close":
            depth += 1
        elif toks[j].kind == "open":
            depth -= 1
            if depth == 0:
                return j
    return -1


PLACEHOLDER_RE = re.compile(r"\{\{|\}\}|\{([^{}]*)\}")


def bad_spec(fmt: str) -> str | None:
    """The first placeholder of `fmt` that sets a precision or an exponent type, if any."""
    for m in PLACEHOLDER_RE.finditer(fmt):
        inner = m.group(1)
        if inner is None or ":" not in inner:
            continue
        spec = inner.split(":", 1)[1]
        if "." in spec or spec.endswith(("e", "E")):
            return m.group(0)
    return None


def floaty(tokens: list[Tok]) -> Tok | None:
    for t in tokens:
        if t.kind == "float" or (t.kind == "ident" and t.text in FLOAT_TYPES):
            return t
    return None


def float_allowed(path: str) -> bool:
    return path.startswith(ALLOWED_FLOAT_FILES)


def scan_tokens(path: str, toks: list[Tok]) -> list[Finding]:
    out: list[Finding] = []
    allowed = float_allowed(path)

    def add(t: Tok, rule: str, msg: str) -> None:
        out.append(Finding(path, t.line, t.col, rule, msg))

    for k, t in enumerate(toks):
        if not allowed and t.kind == "float":
            add(t, "RPT-FLOAT-LITERAL", f"float literal `{t.text}` outside fid_report::fmt")
        if t.kind != "ident":
            continue
        nxt0 = toks[k + 1] if k + 1 < len(toks) else None
        if not allowed and t.text in FLOAT_TYPES:
            add(t, "RPT-FLOAT-TYPE", f"`{t.text}` outside fid_report::fmt")
        if not allowed and t.text in ("allow", "expect", "warn") and nxt0 is not None and nxt0.text == "(":
            c = match_close(toks, k + 1)
            for j in range(k + 2, c):
                g = toks[j]
                if g.kind != "ident" or g.text not in LINT_SWITCHES:
                    continue
                is_clippy = j >= 2 and toks[j - 1].text == "::" and toks[j - 2].text == "clippy"
                if g.text == "warnings" or is_clippy:
                    add(g, "RPT-LINT-ALLOW", f"`{t.text}` of `{g.text}` would switch off the float type rule")
        nxt = toks[k + 1] if k + 1 < len(toks) else None
        prev = toks[k - 1] if k > 0 else None
        # Formatting macro invocation: name ! ( ... )
        if (
            t.text in FORMAT_MACROS
            and nxt is not None
            and nxt.text == "!"
            and k + 2 < len(toks)
            and toks[k + 2].kind == "open"
            and not (prev is not None and prev.text == "macro_rules")
        ):
            o = k + 2
            c = match_close(toks, o)
            args = toks[o + 1 : c]
            first = next((j for j, a in enumerate(args) if a.kind == "str"), None)
            if first is not None:
                spec = bad_spec(args[first].text)
                if spec is not None:
                    add(args[first], "RPT-FLOAT-SPEC", f"`{t.text}!` format spec `{spec}` sets a precision or exponent")
                # Tokens before the format string are a writer or an assertion's condition.
                args = args[first + 1 :]
            elif t.text in ASSERT_MACROS:
                continue  # no message: nothing is formatted
            f = floaty(args)
            if f is not None:
                add(f, "RPT-FLOAT-ARG", f"`{t.text}!` argument `{f.text}` is a float")
            continue
        if t.text in FLOAT_TRAITS:
            add(t, "RPT-FLOAT-TRAIT", f"`{t.text}` formats floats")
            continue
        if t.text in FLOAT_CRATES and nxt is not None and nxt.text == "::":
            add(t, "RPT-FLOAT-CRATE", f"`{t.text}` is a float-printing crate")
            continue
        if t.text == "to_string":
            if prev is not None and prev.text == "::" and k >= 2 and toks[k - 2].text in FLOAT_TYPES:
                add(t, "RPT-FLOAT-TOSTRING", f"`{toks[k - 2].text}::to_string`")
                continue
            if prev is not None and prev.text == "." and k >= 2:
                r = toks[k - 2]
                if r.kind == "float" or (r.kind == "ident" and r.text in FLOAT_TYPES):
                    add(t, "RPT-FLOAT-TOSTRING", f"`.to_string()` on `{r.text}`")
                elif r.kind == "close":
                    o = match_open(toks, k - 2)
                    f = floaty(toks[o + 1 : k - 2]) if o >= 0 else None
                    if f is not None:
                        add(t, "RPT-FLOAT-TOSTRING", f"`.to_string()` on a group with `{f.text}`")
            # `ToString::to_string(&1.5)` and `f64::to_string(&x)` style calls.
            if nxt is not None and nxt.kind == "open" and nxt.text == "(":
                c = match_close(toks, k + 1)
                f = floaty(toks[k + 2 : c])
                if f is not None:
                    add(t, "RPT-FLOAT-TOSTRING", f"`to_string` of `{f.text}`")
    return out


def scan_file(path: Path, rel: str) -> list[Finding]:
    try:
        src = path.read_bytes().decode("utf-8")
    except UnicodeDecodeError:
        return [Finding(rel, 1, 1, "RPT-LEX", "not UTF-8")]
    try:
        toks = lex(src)
    except LexError as e:
        return [Finding(rel, e.line, e.col, "RPT-LEX", e.what)]
    return scan_tokens(rel, toks)


def crate_dirs(root: Path) -> list[Path]:
    return sorted(p for p in (root / "crates").glob(CRATE_GLOB) if (p / "Cargo.toml").is_file())


def scan_crate(crate: Path, base: Path) -> list[Finding]:
    out: list[Finding] = []
    src = crate / "src"
    for f in sorted(src.rglob("*.rs")):
        try:
            rel = f.relative_to(base).as_posix()
        except ValueError:
            rel = f.as_posix()
        out.extend(scan_file(f, rel))
    return out


def check_config(root: Path) -> list[Finding]:
    """Every fid-report* crate has a clippy.toml disallowing f32 and f64."""
    import tomllib

    out: list[Finding] = []
    for c in crate_dirs(root):
        rel = (c / "clippy.toml").relative_to(root).as_posix()
        try:
            cfg = tomllib.loads((c / "clippy.toml").read_text(encoding="utf-8"))
        except (OSError, ValueError) as e:
            out.append(Finding(rel, 1, 1, "RPT-CONFIG", f"unreadable: {e.__class__.__name__}"))
            continue
        paths = set()
        for entry in cfg.get("disallowed-types", []):
            paths.add(entry["path"] if isinstance(entry, dict) else entry)
        missing = sorted(FLOAT_TYPES - paths)
        if missing:
            out.append(Finding(rel, 1, 1, "RPT-CONFIG", f"disallowed-types lacks {', '.join(missing)}"))
    return out


def run(root: Path, crates: list[Path] | None = None) -> list[Finding]:
    targets = crates if crates is not None else crate_dirs(root)
    out: list[Finding] = []
    for c in targets:
        out.extend(scan_crate(c, root))
    if crates is None:
        out.extend(check_config(root))
    return sorted(out, key=lambda f: (f.path, f.line, f.col, f.rule))


def annotated(src: str, tag: str) -> dict[int, list[str]]:
    """Lines annotated `// expect: RULE ...` (tag `expect`) or carrying the word `clippy`
    after `// expect:` (tag `clippy`), 1-based."""
    out: dict[int, list[str]] = {}
    for n, line in enumerate(src.splitlines(), 1):
        m = re.search(r"// expect:(.*)$", line)
        if not m:
            continue
        words = m.group(1).split()
        if tag == "clippy":
            if "clippy" in words:
                out[n] = ["clippy"]
        else:
            rules = sorted(w for w in words if w.startswith("RPT-"))
            if rules:
                out[n] = rules
    return out


def clippy_selftest(root: Path) -> int:
    import json
    import os
    import shutil
    import subprocess
    import tempfile

    fixture = root / CLIPPY_FIXTURE
    src = (fixture / "src" / "lib.rs").read_text(encoding="utf-8")
    want = set(annotated(src, "clippy"))
    if not want:
        print("clippy-selftest: the fixture has no `clippy` annotation", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory() as d:
        crate = Path(d) / "crate"
        (crate / "src").mkdir(parents=True)
        shutil.copyfile(fixture / "Cargo.toml.in", crate / "Cargo.toml")
        shutil.copyfile(fixture / "src" / "lib.rs", crate / "src" / "lib.rs")
        shutil.copyfile(root / "crates" / "fid-report" / "clippy.toml", crate / "clippy.toml")
        env = {k: v for k, v in os.environ.items() if not k.startswith(("CARGO_", "CLIPPY_", "RUSTFLAGS"))}
        env["CARGO_TARGET_DIR"] = str(Path(d) / "target")
        try:
            p = subprocess.run(
                ["cargo", "clippy", "--offline", "--quiet", "--message-format=json", "--", "-D", "clippy::disallowed_types"],
                cwd=crate, env=env, capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=600,
            )
        except (OSError, subprocess.TimeoutExpired) as e:
            print(f"clippy-selftest: cannot run cargo clippy: {e}", file=sys.stderr)
            return 1
    got: set[int] = set()
    other: list[str] = []
    for line in p.stdout.splitlines():
        try:
            msg = json.loads(line)
        except ValueError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        m = msg["message"]
        code = (m.get("code") or {}).get("code")
        for sp in m.get("spans", []):
            if sp.get("is_primary") and sp.get("file_name", "").endswith("lib.rs"):
                if code == "clippy::disallowed_types":
                    got.add(sp["line_start"])
                elif m.get("level") == "error" and code not in (None, "clippy::disallowed_types"):
                    other.append(m.get("rendered", ""))
    if p.returncode == 0:
        print("clippy-selftest: clippy passed the bad fixture; the float type rule is not active", file=sys.stderr)
        return 1
    if not got:
        print("clippy-selftest: no disallowed_types diagnostic; stderr:\n" + p.stderr[-4000:], file=sys.stderr)
        return 1
    missed = sorted(want - got)
    for n in missed:
        print(f"{CLIPPY_FIXTURE}/src/lib.rs:{n}: expected clippy::disallowed_types, none reported", file=sys.stderr)
    for r in other:
        print(f"clippy-selftest: unexpected error:\n{r}", file=sys.stderr)
    if missed or other:
        return 1
    print(f"clippy-selftest: disallowed_types fired on all {len(want)} annotated lines", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--root", type=Path, default=ROOT, help="repository root (default: this checkout)")
    ap.add_argument("--crate", type=Path, action="append", help="scan this crate directory instead of crates/fid-report*")
    ap.add_argument("--clippy-selftest", action="store_true", help="prove the clippy layer on the bad fixture crate")
    a = ap.parse_args(argv)
    if a.clippy_selftest:
        return clippy_selftest(a.root.resolve())
    crates = [c.resolve() for c in a.crate] if a.crate else None
    if crates is None and not crate_dirs(a.root):
        print(f"rpt_float_lint: no {CRATE_GLOB} crate under {a.root / 'crates'}", file=sys.stderr)
        return 2
    findings = run(a.root.resolve(), crates)
    for f in findings:
        print(f)
    scanned = len(crates) if crates is not None else len(crate_dirs(a.root))
    print(f"rpt_float_lint: {len(findings)} finding(s) in {scanned} crate(s)", file=sys.stderr)
    return 1 if findings else 0


if __name__ == "__main__":
    sys.exit(main())
