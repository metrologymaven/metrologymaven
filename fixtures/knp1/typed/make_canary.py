#!/usr/bin/env python3
"""Independent renderer of the KRN.NUM.LINT.TYPED canary (the kind guard).

The canary crate `fixtures/knp1/typed/canary/` uses every row of the pinned table exactly once,
one row per source line, through a hand-written snippet per path below. Rows that only exist on
some architectures (the feature-detection macros) carry `#[cfg(target_arch = ...)]` so the
crate compiles for every kernel target. The script writes:

- `canary/src/lib.rs`: the crate source; each row's line ends with `// row NNN: RULE kind path`;
- `canary/expected-hits.txt`: one line `TARGET RULE KIND LINE` per row the driver must report on
  that target, and one line `syn RULE KIND LINE` per row that the syntax scan
  (KRN.NUM.LINT.SYN) guards instead because clippy cannot see it (SYN_GUARDED), sorted. Every
  row has at least one line: every row is guarded by some lint.

Clippy ignores a `clippy.toml` entry listed under the wrong kind (a type under
`disallowed-methods`, say) without a word; such a row then shows up as a missing hit.

The rows come from the independent table in make_expected.py (itself written by hand from
spec/krn/num/lint/typed/NODE.md); the snippets and the per-target expectations are written here
by hand. Stdlib only; it never runs the Rust code.

Usage: python fixtures/knp1/typed/make_canary.py [--check]
"""

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True

import make_expected  # noqa: E402  (same directory; the independent table)

TARGETS = [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "wasm32-unknown-unknown",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
]

# Arguments of each float method call; methods not listed take none.
FLOAT_ARGS = {
    "mul_add": "x, x", "powf": "x", "powi": "2", "atan2": "x", "hypot": "x", "log": "x",
    "clamp": "0.0, 1.0", "max": "x", "min": "x",
}

P = "p: &std::path::Path"
FS = {
    "canonicalize": (P, "p"), "copy": (P, "p, p"), "create_dir": (P, "p"), "create_dir_all": (P, "p"),
    "exists": (P, "p"), "hard_link": (P, "p, p"), "metadata": (P, "p"), "read": (P, "p"),
    "read_dir": (P, "p"), "read_link": (P, "p"), "read_to_string": (P, "p"), "remove_dir": (P, "p"),
    "remove_dir_all": (P, "p"), "remove_file": (P, "p"), "rename": (P, "p, p"),
    "set_permissions": (P + ", q: std::fs::Permissions", "p, q"), "symlink_metadata": (P, "p"),
    "write": (P, "p, b\"\""),
}
ENV = {
    "args": ("", ""), "args_os": ("", ""), "current_dir": ("", ""), "current_exe": ("", ""),
    "home_dir": ("", ""), "join_paths": (P, "[p]"), "remove_var": ("", "\"KNP1_CANARY\""),
    "set_current_dir": (P, "p"), "set_var": ("", "\"KNP1_CANARY\", \"1\""), "split_paths": ("", "\"a\""),
    "temp_dir": ("", ""), "var": ("", "\"KNP1_CANARY\""), "var_os": ("", "\"KNP1_CANARY\""), "vars": ("", ""),
    "vars_os": ("", ""),
}
D = "d: std::time::Duration"
THREAD = {
    "available_parallelism": ("", ""), "current": ("", ""), "park": ("", ""), "park_timeout": (D, "d"),
    "scope": ("", "|_| {}"), "sleep": (D, "d"), "spawn": ("", "|| {}"), "yield_now": ("", ""),
}
PROCESS = {"abort": ("", ""), "exit": ("", "0"), "id": ("", "")}

# A type that also has a method row is used through a type alias: the alias line is the type
# row, and the method row calls through the alias, so neither line holds a second disallowed item.
ALIASES = {
    "std::time::Instant": "CanaryInstant",
    "std::time::SystemTime": "CanarySystemTime",
    "std::process::Command": "CanaryCommand",
}
GENERIC_ARGS = {"std::collections::HashMap": "<u8, u8>", "std::collections::HashSet": "<u8>"}

# Macro rows: (cfg attribute or "", target architectures or None for all, code).
MACROS = {
    "std::arch::is_aarch64_feature_detected": (
        '#[cfg(target_arch = "aarch64")] ', ["aarch64"], 'let _ = std::arch::is_aarch64_feature_detected!("neon");'),
    "std::is_x86_feature_detected": (
        '#[cfg(target_arch = "x86_64")] ', ["x86_64"], 'let _ = std::is_x86_feature_detected!("sse2");'),
    "std::env": ("", None, 'let _ = std::env!("CARGO_PKG_NAME");'),
    "std::option_env": ("", None, 'let _ = std::option_env!("KNP1_CANARY");'),
    "std::include": ("", None, 'let _: u8 = std::include!("inc.in");'),
    "std::include_bytes": ("", None, 'let _ = std::include_bytes!("inc.txt");'),
    "std::include_str": ("", None, 'let _ = std::include_str!("inc.txt");'),
}


# Rows the driver never reports, guarded by the syntax scan instead. Clippy does not report
# `std::include!` in expression position on any kernel target (CI, PR #288). KRN.NUM.LINT.SYN
# reports every `include!` whose argument is not a single plain literal naming a `.rs` file the
# scan lists (KNP1-IO-FS); `inc.in` is not one. An `include!` of a listed `.rs` file is scanned
# like a module, so no unscanned file is read.
SYN_GUARDED = {"std::include"}


def fn(n: int, params: str, body: str, attr: str = "") -> str:
    return f"{attr}pub fn r{n:03}({params}) {{ {body} }}"


def method_line(n: int, path: str) -> str:
    head, _, name = path.rpartition("::")
    if head in ("f32", "f64"):
        return fn(n, f"x: {head}", f"let _ = x.{name}({FLOAT_ARGS.get(name, '')});")
    if head == "std::fs":
        params, args = FS[name]
        return fn(n, params, f"let _ = std::fs::{name}({args});")
    if head == "std::env":
        params, args = ENV[name]
        attr = "#[allow(deprecated)] " if name == "home_dir" else ""
        return fn(n, params, f"let _ = std::env::{name}({args});", attr)
    if head == "std::thread":
        params, args = THREAD[name]
        return fn(n, params, f"let _ = std::thread::{name}({args});")
    if head == "std::process":
        params, args = PROCESS[name]
        return fn(n, params, f"let _ = std::process::{name}({args});")
    if head in ALIASES:
        arg = '"knp1"' if name == "new" else ""
        return fn(n, "", f"let _ = {ALIASES[head]}::{name}({arg});")
    raise SystemExit(f"no canary snippet for method {path}")


def type_line(n: int, path: str) -> str:
    if path in ALIASES:
        return f"pub type {ALIASES[path]} = {path};"
    return fn(n, "", f"let _: Option<{path}{GENERIC_ARGS.get(path, '')}> = None;")


def macro_line(n: int, path: str):
    if path == "std::thread_local":
        return "std::thread_local! { static CANARY: u8 = 0; }", None
    if path not in MACROS:
        raise SystemExit(f"no canary snippet for macro {path}")
    attr, archs, body = MACROS[path]
    return fn(n, "", body, attr), archs


HEADER = [
    "//! KRN.NUM.LINT.TYPED kind guard: every row of the pinned table used exactly once, one row per",
    "//! line, each line naming its row. Generated by fixtures/knp1/typed/make_canary.py; do not edit.",
    "#![allow(deprecated, unused, clippy::all)]",
    "",
]


def render():
    rows = ([(r, "method", p) for p, r in make_expected.METHODS]
            + [(r, "type", p) for p, r in make_expected.TYPES]
            + [(r, "macro", p) for p, r in make_expected.MACROS])
    if len(rows) != 132 or len({(k, p) for _, k, p in rows}) != len(rows):
        raise SystemExit(f"unexpected table: {len(rows)} rows")
    src = list(HEADER)
    hits = []
    for i, (rule, kind, path) in enumerate(rows, start=1):
        archs = None
        if kind == "method":
            code = method_line(i, path)
        elif kind == "type":
            code = type_line(i, path)
        else:
            code, archs = macro_line(i, path)
        src.append(f"{code} // row {i:03}: {rule} {kind} {path}")
        line = len(src)
        if path in SYN_GUARDED:
            hits.append(f"syn {rule} {kind} {line}")
            continue
        for t in TARGETS:
            if archs is None or t.split("-")[0] in archs:
                hits.append(f"{t} {rule} {kind} {line}")
    return "\n".join(src) + "\n", "\n".join(sorted(hits)) + "\n"


def main() -> int:
    lib, hits = render()
    outputs = {HERE / "canary" / "src" / "lib.rs": lib, HERE / "canary" / "expected-hits.txt": hits}
    if "--check" in sys.argv[1:]:
        bad = [p for p, t in outputs.items() if not p.exists() or p.read_bytes() != t.encode("utf-8")]
        for p in bad:
            print(f"FAIL: {p} is stale; run without --check")
        if not bad:
            print("typed canary: ok")
        return 1 if bad else 0
    for p, t in outputs.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(t.encode("utf-8"))
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
