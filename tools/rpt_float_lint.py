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
   - RPT-NUM: `CValue::Num` used other than as the wildcard pattern `CValue::Num(_)`, or
     `Num` / `CValue::*` imported, outside the allowed files (the JSON number of
     `fid-canon` is a float reached by inference);
   - RPT-CONFIG: a `fid-report*` crate without a `clippy.toml` that disallows both `f32`
     and `f64`, or whose `disallowed-methods` misses a public function of one of its
     direct path dependencies that returns `f32`/`f64` (or an `Option`/`Result` of one;
     source scan, see `dependency_float_fns`); a `.clippy.toml` in a `fid-report*` crate
     or at the root; a `[workspace.lints.clippy]` that sets `disallowed_types`,
     `disallowed_methods`, `style` or `all` to `allow`; a CI workflow that sets
     `CLIPPY_CONF_DIR`.

   Comments and string contents are not code. Integration tests are not shipped and not
   scanned (they may name floats, with `#![allow(clippy::disallowed_types)]`).

`--clippy-selftest` proves layer 1 in CI: it copies the fixture crate
`fixtures/rpt/model/fmt/lint/bad` (whose `src/lib.rs` holds the reviewed bypass cases)
with `crates/fid-report/clippy.toml` into a temporary directory, runs
`cargo clippy -- -D clippy::disallowed_types` on it, and requires a `disallowed_types`
error on every line annotated `clippy` in the fixture. It fails if clippy is missing.

3. Inferred types (third layer, `--mir`). The first two layers do not see a float that
   reaches code purely by type inference, with no float type named, no literal, no listed
   method and no `CValue::Num` binding: a public `f64` field of a dependency's struct, a
   `Vec<f64>` or slice, a trait method, a transitive dependency's type. `--mir` closes that
   gap with the compiler's own types: for every `crates/fid-report*` crate (all features,
   library target) it asks stable rustc for its MIR (`cargo rustc -- --emit=mir`), where
   every local, temporary, argument and constant carries its inferred type, and reports
   RPT-FLOAT-MIR for each function, closure, constant or promoted body outside the allowed
   files whose MIR names `f32` or `f64` anywhere (types, constants such as `2f64`, calls
   such as `<impl f64>::from_bits`). A body is attributed to its source file by the
   `<impl at FILE:...>` span rustc prints, by an explicit module path, or (rustc trims
   unique paths to the bare name) by the files of the crate that define that name; a name
   that resolves to no file, or to any file outside the allowed ones, is reported (fail
   closed). Compiler shims for foreign items (`std::`, `core::`, `alloc::`) are skipped:
   they exist only for a local type, whose own body is scanned. `--mir-selftest` proves
   the layer on the fixture crate `fixtures/rpt/model/fmt/lint/infer` (with its dependency
   `dep`): every function annotated `// mir` must be reported, and nothing else.

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
MIR_FIXTURE = "fixtures/rpt/model/fmt/lint/infer"
# Crate-relative files where a float may appear in MIR, per crate directory name.
MIR_ALLOWED = {"fid-report": ("src/fmt/", "src/json_num.rs")}
MIR_FLOAT = re.compile(r"(?<![A-Za-z_])f(?:32|64)\b")
FOREIGN_PREFIXES = ("std::", "core::", "alloc::")
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


def in_use(toks: list[Tok], k: int) -> bool:
    """Whether token k lies in a `use` declaration (back to the previous `;`, `{` or `}`
    at statement level)."""
    depth = 0
    for j in range(k - 1, -1, -1):
        t = toks[j]
        if t.kind == "close":
            depth += 1
        elif t.kind == "open":
            if depth == 0 and t.text != "{":
                continue
            if depth == 0:
                # `{` of a use tree (`use a::{b, c}`) or of a block: look further back.
                continue
            depth -= 1
        elif t.text == ";" and depth == 0:
            return False
        elif t.kind == "ident" and t.text == "use" and depth == 0:
            return True
    return False


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
        if not allowed and t.text == "Num":
            pattern = (
                k >= 2
                and toks[k - 1].text == "::"
                and toks[k - 2].text == "CValue"
                and [x.text for x in toks[k + 1 : k + 4]] == ["(", "_", ")"]
                and not in_use(toks, k)
            )
            if not pattern:
                add(t, "RPT-NUM", "`CValue::Num` only as the pattern `CValue::Num(_)` outside fid_report::fmt")
        if not allowed and t.text == "CValue" and [x.text for x in toks[k + 1 : k + 3]] == ["::", "*"]:
            add(t, "RPT-NUM", "glob import of `CValue::*`")
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


FN_RE = re.compile(
    r"^(?P<indent>[ \t]*)pub\s+(?:const\s+)?(?:unsafe\s+)?fn\s+(?P<name>\w+)(?P<sig>[^{;]*)",
    re.M,
)
FLOAT_RET_RE = re.compile(r"->\s*(?:f32|f64|Option<\s*f(?:32|64)\s*>|Result<\s*f(?:32|64)\s*[,>])")
IMPL_RE = re.compile(r"^impl(?:<[^{]*?>)?\s+(?P<ty>[A-Za-z_]\w*)(?P<rest>[^{]*)\{", re.M)


def _module_file(src: Path, chain: list[str]) -> Path | None:
    if not chain:
        return src / "lib.rs"
    base = src.joinpath(*chain)
    for f in (base.with_suffix(".rs"), base / "mod.rs"):
        if f.is_file():
            return f
    return None


def _chain_of(src: Path, f: Path) -> list[str]:
    parts = list(f.relative_to(src).with_suffix("").parts)
    if parts[-1] in ("lib", "mod"):
        parts.pop()
    return parts


def dependency_float_fns(crate_dir: Path) -> list[list[str]]:
    """Public functions and inherent methods of `crate_dir`'s src that return f32/f64 or an
    Option/Result of one. Each entry is the list of clippy paths that name it: the
    definition path through public modules, plus each path through a `pub use` of the
    item or its type in an ancestor module (a source scan; good enough for this
    workspace's style, fail closed on what it cannot place)."""
    src = crate_dir / "src"
    krate = crate_dir.name.replace("-", "_")
    out: list[list[str]] = []
    for f in sorted(src.rglob("*.rs")):
        text = f.read_text(encoding="utf-8")
        chain = _chain_of(src, f)
        if "tests" in chain or "test" in chain:
            continue
        for m in FN_RE.finditer(text):
            if not FLOAT_RET_RE.search(m.group("sig")):
                continue
            name = m.group("name")
            owner = None
            if m.group("indent"):
                impls = [i for i in IMPL_RE.finditer(text, 0, m.start())]
                if not impls or " for " in impls[-1].group("rest"):
                    continue  # trait impl or nested: not an inherent public method
                owner = impls[-1].group("ty")
            item = owner or name
            tail = [owner, name] if owner else [name]
            paths = []
            # Walk from the defining module up: keep a module segment when its parent
            # declares it `pub mod`; else the item must be re-exported by the parent.
            for cut in range(len(chain), -1, -1):
                prefix = chain[:cut]
                if cut < len(chain):
                    parent = _module_file(src, prefix)
                    ptext = parent.read_text(encoding="utf-8") if parent else ""
                    if not re.search(r"pub\s+use\s[^;]*\b" + re.escape(item) + r"\b", ptext):
                        if not re.search(r"pub\s+mod\s+" + re.escape(chain[cut]) + r"\b", ptext):
                            break
                        continue
                visible = all(
                    re.search(
                        r"pub\s+mod\s+" + re.escape(prefix[i]) + r"\b",
                        (_module_file(src, prefix[:i]) or src / "lib.rs").read_text(encoding="utf-8"),
                    )
                    for i in range(len(prefix))
                )
                if visible:
                    paths.append("::".join([krate] + prefix + tail))
            out.append(paths or ["::".join([krate] + chain + tail) + " (unplaceable)"])
    return out


def path_deps(crate_dir: Path, root: Path) -> list[Path]:
    import tomllib

    manifest = tomllib.loads((crate_dir / "Cargo.toml").read_text(encoding="utf-8"))
    out = []
    for name, spec in sorted(manifest.get("dependencies", {}).items()):
        if isinstance(spec, dict) and "path" in spec:
            out.append((crate_dir / spec["path"]).resolve())
    return out


def check_config(root: Path) -> list[Finding]:
    """The clippy layer is configured, complete and not weakened (see RPT-CONFIG)."""
    import tomllib

    out: list[Finding] = []
    for name in (".clippy.toml",):
        if (root / name).exists():
            out.append(Finding(name, 1, 1, "RPT-CONFIG", "a root .clippy.toml would shadow the crate configs"))
    ws = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8")).get("workspace", {})
    lints = ws.get("lints", {}).get("clippy", {})
    for key in ("disallowed_types", "disallowed_methods", "style", "all"):
        level = lints.get(key)
        level = level.get("level") if isinstance(level, dict) else level
        if level == "allow":
            out.append(Finding("Cargo.toml", 1, 1, "RPT-CONFIG", f"[workspace.lints.clippy] sets {key} to allow"))
    wf_dir = root / ".github" / "workflows"
    for wf in sorted(wf_dir.glob("*.yml")) if wf_dir.is_dir() else []:
        if "CLIPPY_CONF_DIR" in wf.read_text(encoding="utf-8"):
            out.append(Finding(wf.relative_to(root).as_posix(), 1, 1, "RPT-CONFIG", "sets CLIPPY_CONF_DIR"))
    for c in crate_dirs(root):
        if (c / ".clippy.toml").exists():
            rel = (c / ".clippy.toml").relative_to(root).as_posix()
            out.append(Finding(rel, 1, 1, "RPT-CONFIG", "a .clippy.toml would take precedence over clippy.toml"))
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
        methods = set()
        for entry in cfg.get("disallowed-methods", []):
            methods.add(entry["path"] if isinstance(entry, dict) else entry)
        if (c / "Cargo.toml").is_file():
            for dep in path_deps(c, root):
                if not (dep / "src").is_dir():
                    continue
                for names in dependency_float_fns(dep):
                    if not methods.intersection(names):
                        out.append(Finding(rel, 1, 1, "RPT-CONFIG", f"disallowed-methods lacks {names[0]}"))
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


# ------------------------------------------------------------------ layer 3: MIR


@dataclass(frozen=True)
class MirItem:
    header: str
    name: str
    body: str


def mir_items(text: str) -> list[MirItem]:
    """The bodies of a `--emit=mir` file: each starts at a column-0 line that is not a
    comment and ends at the column-0 `}` (or is one line, for a `const` without a body)."""
    items: list[MirItem] = []
    lines = text.splitlines()
    i = 0
    while i < len(lines):
        line = lines[i]
        if not line or line.startswith((" ", "//", "}")):
            i += 1
            continue
        start = i
        if line.rstrip().endswith("{"):
            while i < len(lines) and lines[i] != "}":
                i += 1
        body = "\n".join(lines[start : i + 1])
        items.append(MirItem(line, mir_name(line), body))
        i += 1
    return items


def mir_name(header: str) -> str:
    """The item path of a MIR header: `fn PATH(`, `const PATH: T`, `static PATH: T` or
    `promoted[n] in PATH: T`."""
    h = header
    m = re.match(r"promoted\[\d+\] in (.*)$", h)
    if m:
        h = "fn " + m.group(1)
    for kw in ("fn ", "const ", "static mut ", "static "):
        if h.startswith(kw):
            h = h[len(kw) :]
            break
    depth = 0
    for k, c in enumerate(h):
        if c in "<{[":
            depth += 1
        elif c in ">}]":
            depth -= 1
        elif depth == 0 and (c == "(" or c == ":" and h[k : k + 2] != "::" and h[k - 1 : k + 1] != "::"):
            return h[:k].strip()
    return h.strip()


def _definers(src_files: dict[str, str], ident: str) -> list[str]:
    pat = re.compile(r"\b(?:fn|struct|enum|union|const|static|type|trait|mod)\s+" + re.escape(ident) + r"\b")
    variant = re.compile(r"^\s*" + re.escape(ident) + r"\s*[({,]", re.M)
    return sorted(f for f, t in src_files.items() if pat.search(t) or variant.search(t))


def mir_attribution(name: str, src_files: dict[str, str]) -> list[str] | None:
    """The crate-relative files `name` may belong to (`None`: a foreign shim, skipped)."""
    if name.startswith(FOREIGN_PREFIXES):
        return None
    if name.startswith("<") and not name.startswith("<impl at ") and name[1:].startswith(FOREIGN_PREFIXES):
        return None
    m = re.search(r"<impl at ([^:>]+(?::[A-Za-z]:)?[^:>]*):\d+:\d+", name)
    if m:
        return [m.group(1).replace("\\", "/")]
    segs = [g for g in re.sub(r"<[^<>]*>", "", name).split("::") if g and not g.startswith("{")]
    if len(segs) >= 2:
        return ["mod:" + segs[0]]
    return _definers(src_files, segs[0]) if segs else []


def _allowed(where: str, crate_rel: str, allowed: tuple[str, ...]) -> bool:
    if where.startswith("mod:"):
        mod = where[4:]
        return any(a.rstrip("/").removesuffix(".rs").endswith("src/" + mod) for a in allowed)
    w = where
    if crate_rel and (w.startswith(crate_rel + "/")):
        w = w[len(crate_rel) + 1 :]
    return any(w.startswith(a) for a in allowed)


def mir_findings(text: str, crate_dir: Path, crate_rel: str, allowed: tuple[str, ...]) -> list[Finding]:
    """RPT-FLOAT-MIR for each body outside `allowed` whose MIR names a float type."""
    src = crate_dir / "src"
    files = {
        f.relative_to(crate_dir).as_posix(): f.read_text(encoding="utf-8", errors="replace")
        for f in sorted(src.rglob("*.rs"))
    }
    out: list[Finding] = []
    seen: set[str] = set()
    for it in mir_items(text):
        if not MIR_FLOAT.search(it.body) or it.name in seen:
            continue
        where = mir_attribution(it.name, files)
        if where is None:
            continue
        if where and all(_allowed(w, crate_rel, allowed) for w in where):
            continue
        seen.add(it.name)
        place = ", ".join(where) if where else "no source file"
        out.append(Finding(f"{crate_rel}/src", 0, 0, "RPT-FLOAT-MIR", f"{it.name} ({place}) has a float in its MIR"))
    return out


def emit_mir(crate_dir: Path, cwd: Path, out: Path, package: str, target_dir: Path | None = None) -> str:
    import os
    import subprocess

    env = {k: v for k, v in os.environ.items() if not k.startswith(("RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"))}
    if target_dir is not None:
        env["CARGO_TARGET_DIR"] = str(target_dir)
    cmd = ["cargo", "rustc", "--quiet", "--lib", "--all-features", "-p", package, "--", f"--emit=mir={out}"]
    p = subprocess.run(cmd, cwd=cwd, env=env, capture_output=True, text=True, stdin=subprocess.DEVNULL, timeout=1800)
    if p.returncode != 0 or not out.exists():
        raise RuntimeError(f"cargo rustc --emit=mir failed for {package}:\n{p.stderr[-4000:]}")
    return out.read_text(encoding="utf-8", errors="replace")


def _package_name(crate_dir: Path) -> str:
    for line in (crate_dir / "Cargo.toml").read_text(encoding="utf-8").splitlines():
        m = re.match(r'\s*name\s*=\s*"([^"]+)"', line)
        if m:
            return m.group(1)
    return crate_dir.name


def mir_check(root: Path) -> int:
    import tempfile

    findings: list[Finding] = []
    with tempfile.TemporaryDirectory() as d:
        for c in crate_dirs(root):
            rel = c.relative_to(root).as_posix()
            try:
                text = emit_mir(c, root, Path(d) / f"{c.name}.mir", _package_name(c))
            except (OSError, RuntimeError) as e:
                print(f"rpt_float_lint --mir: {e}", file=sys.stderr)
                return 1
            findings.extend(mir_findings(text, c, rel, MIR_ALLOWED.get(c.name, ())))
    for f in findings:
        print(f)
    print(f"rpt_float_lint --mir: {len(findings)} finding(s) in {len(crate_dirs(root))} crate(s)", file=sys.stderr)
    return 1 if findings else 0


def mir_selftest(root: Path) -> int:
    import shutil
    import tempfile

    fixture = root / MIR_FIXTURE
    src = (fixture / "src" / "lib.rs").read_text(encoding="utf-8")
    want = {m.group(1) for m in re.finditer(r"fn (\w+)[^\n]*// mir\b", src)}
    if not want:
        print("mir-selftest: the fixture has no `// mir` annotation", file=sys.stderr)
        return 1
    with tempfile.TemporaryDirectory() as d:
        crate = Path(d) / "crate"
        shutil.copytree(fixture, crate)
        for m in crate.rglob("Cargo.toml.in"):
            m.rename(m.with_name("Cargo.toml"))
        try:
            text = emit_mir(crate, crate, Path(d) / "infer.mir", "rpt-float-rule-infer-fixture", Path(d) / "target")
        except (OSError, RuntimeError) as e:
            print(f"mir-selftest: {e}", file=sys.stderr)
            return 1
        found = mir_findings(text, crate, "", ("src/fmt/",))
    got = {re.split(r"::|\s", f.message, maxsplit=1)[0] for f in found}
    missed, extra = sorted(want - got), sorted(got - want)
    for n in missed:
        print(f"mir-selftest: `{n}` has an inferred float and was not reported", file=sys.stderr)
    for n in extra:
        print(f"mir-selftest: `{n}` was reported but is not annotated", file=sys.stderr)
    if missed or extra:
        return 1
    print(f"mir-selftest: the MIR layer reported all {len(want)} annotated functions and nothing else", file=sys.stderr)
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    ap.add_argument("--root", type=Path, default=ROOT, help="repository root (default: this checkout)")
    ap.add_argument("--crate", type=Path, action="append", help="scan this crate directory instead of crates/fid-report*")
    ap.add_argument("--clippy-selftest", action="store_true", help="prove the clippy layer on the bad fixture crate")
    ap.add_argument("--float-fns", type=Path, help="print the float-returning public functions of a crate directory")
    ap.add_argument("--mir", action="store_true", help="layer 3: scan the MIR of every fid-report* crate for floats")
    ap.add_argument("--mir-selftest", action="store_true", help="prove the MIR layer on the infer fixture crate")
    a = ap.parse_args(argv)
    if a.mir:
        return mir_check(a.root.resolve())
    if a.mir_selftest:
        return mir_selftest(a.root.resolve())
    if a.float_fns:
        for names in dependency_float_fns(a.float_fns.resolve()):
            print(" | ".join(names))
        return 0
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
