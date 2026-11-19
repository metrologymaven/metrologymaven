#!/usr/bin/env python3
"""Writes the hand-made cargo message streams the KRN.NUM.LINT.TYPED parser tests read.

Each stream imitates `cargo clippy --message-format=json` output; `@WS@` stands for the
absolute fixture workspace path (the test substitutes it). The expected results are asserted
by hand in crates/fid-knp1-lint/tests/typed.rs. Stdlib only.

Usage: python fixtures/knp1/typed/make_messages.py [--check]
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
WS = "@WS@"


def artifact(crate: str, kind: str = "lib") -> dict:
    return {"reason": "compiler-artifact", "package_id": crate, "manifest_path": f"{WS}/{crate}/Cargo.toml",
            "target": {"kind": [kind], "name": crate}, "fresh": False}


def span(f, l, c, primary=True, exp=None):
    return {"file_name": f, "byte_start": 0, "byte_end": 1, "line_start": l, "line_end": l, "column_start": c,
            "column_end": c + 1, "is_primary": primary, "text": [], "label": None, "suggested_replacement": None,
            "suggestion_applicability": None,
            "expansion": ({"span": exp, "macro_decl_name": "fused!", "def_site_span": None} if exp else None)}


def msg(manifest, text, code, level, spans, notes=()):
    return {"reason": "compiler-message", "package_id": "x", "manifest_path": manifest, "target": {"kind": ["lib"]},
            "message": {"message": text, "code": ({"code": code, "explanation": None} if code else None),
                        "level": level, "spans": spans,
                        "children": [{"message": n, "code": None, "level": "note", "spans": [], "children": [],
                                      "rendered": None} for n in notes],
                        "rendered": text}}


def line(x) -> str:
    return json.dumps(x, sort_keys=True)


MAIN = [
    artifact("macro"),
    artifact("minmax-var"),
    # A build script artifact does not count as the crate being linted.
    artifact("clean", "custom-build"),
    msg(f"{WS}/macro/Cargo.toml", "use of a disallowed method `f64::mul_add`", "clippy::disallowed_methods",
        "warning", [span("macro/src/lib.rs", 5, 12, True, span("macro/src/lib.rs", 20, 5, False))], ["KNP1-FMA"]),
    msg(f"{WS}/minmax-var/Cargo.toml", "use of a disallowed method `f64::min`", "clippy::disallowed_methods",
        "warning", [span(f"{WS}/minmax-var/src/lib.rs", 9, 37)], ["KNP1-MINMAX"]),
    msg(f"{WS}/minmax-var/Cargo.toml", "use of a disallowed method `f64::max`", "clippy::disallowed_methods",
        "warning", [span("/rustc/0000/library/core/src/macros/mod.rs", 1, 1, True,
                         span("minmax-var/src/lib.rs", 5, 15, False))], ["reason: KNP1-MINMAX (pinned)"]),
    # A diagnostic whose spans all lie outside the crate: KNP1-TYPED-NOSPAN.
    msg(f"{WS}/macro/Cargo.toml", "use of a disallowed method `f64::sqrt`", "clippy::disallowed_methods",
        "warning", [span("/rustc/0000/library/std/src/f64.rs", 7, 3)], ["KNP1-LIBM"]),
    msg(f"{WS}/macro/Cargo.toml", "unused variable: `q`", "unused_variables", "warning",
        [span("macro/src/lib.rs", 19, 10)]),
    msg(f"{WS}/nowasm/Cargo.toml", "typed-nowasm does not build for wasm32", None, "error",
        [span("nowasm/src/lib.rs", 4, 1)]),
    msg("/elsewhere/other/Cargo.toml", "use of a disallowed method `f64::sin`", "clippy::disallowed_methods",
        "warning", [span("/elsewhere/other/src/lib.rs", 1, 1)], ["KNP1-LIBM"]),
    # The aarch64 detection macro does not exist on x86-64 or wasm32: allowed there.
    msg(f"{WS}/macro/Cargo.toml", "`std::arch::is_aarch64_feature_detected` does not refer to an existing macro",
        None, "warning", []),
    {"reason": "build-finished", "success": False},
]

OUTPUTS = {
    "messages.jsonl": "".join(line(x) + "\n" for x in MAIN),
    "messages-unmapped.jsonl": line(msg(f"{WS}/macro/Cargo.toml", "use of a disallowed method `f64::exp`",
                                        "clippy::disallowed_methods", "warning", [span("macro/src/lib.rs", 5, 12)],
                                        ["no rule here"])) + "\n",
    "messages-garbage.jsonl": line(artifact("macro")) + "\nthis line is not a cargo message\n",
    "messages-unresolved.jsonl": line(msg(f"{WS}/macro/Cargo.toml",
                                          "`f64::sin` does not refer to an existing function", None, "warning",
                                          [])) + "\n",
    # #235 follow-up: an ordinary E0277 error uses the "expected a …, found …" wording of a driver
    # configuration warning. With a code and a span in the crate it is a compile error of that
    # crate (KNP1-TYPED-BUILD), not a fatal driver error. The same wording with no code and no
    # span in a crate (second stream) is still a driver configuration warning.
    "messages-e0277.jsonl": "".join(line(x) + "\n" for x in [
        artifact("macro"),
        msg(f"{WS}/e0277/Cargo.toml", "expected an `FnMut(f64)` closure, found `f64`", "E0277", "error",
            [span("e0277/src/lib.rs", 6, 27)]),
        msg(f"{WS}/e0277/Cargo.toml", "aborting due to 1 previous error", None, "error", []),
        {"reason": "build-finished", "success": False},
    ]),
    # #288 review: the driver wording with no code but a span inside a linted crate is not a
    # driver configuration warning; as an error it is a compile error of that crate.
    "messages-nocode-incrate.jsonl": line(msg(f"{WS}/macro/Cargo.toml",
                                              "expected a function, found struct `std::knp1_in_crate`", None,
                                              "error", [span("macro/src/lib.rs", 20, 5)])) + "\n",
    "messages-wrong-kind.jsonl": line(msg(f"{WS}/macro/Cargo.toml",
                                          "expected a function, found struct `std::knp1_wrong_kind`", None,
                                          "warning", [])) + "\n",
}


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for rel, text in OUTPUTS.items():
        p = HERE / rel
        data = text.encode("utf-8")
        if check:
            if not p.exists() or p.read_bytes() != data:
                print(f"FAIL: {rel} is stale")
                bad += 1
        else:
            p.write_bytes(data)
    print("typed message fixtures:", "stale" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
