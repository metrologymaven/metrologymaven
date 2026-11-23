#!/usr/bin/env python3
"""Deterministic generator for fixtures/rpt/balloon/auto/callouts_corpus.json (CC-BY-4.0).

Generates 100 synthetic engineering drawing callouts covering all dimension, tolerance,
and GD&T feature-control-frame forms plus title-block distractors for RPT.BALLOON.AUTO.PARSE
(AC-1..AC-4).

Usage:
  python3 fixtures/rpt/balloon/auto/make_corpus.py          # write fixture
  python3 fixtures/rpt/balloon/auto/make_corpus.py --check  # verify byte-for-byte
"""

import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def build_corpus() -> dict:
    # 100 synthetic engineering callouts with exact expected fields (verbatim authored decimals).
    cases = []

    def add(raw, kind, nominal=None, upper=None, lower=None, repeat=1, gdt=None, datums=None, mods=None):
        cases.append(
            {
                "id": len(cases) + 1,
                "raw": raw,
                "kind": kind,
                "repeat_count": repeat,
                "nominal": nominal,
                "upper": upper,
                "lower": lower,
                "gdt_symbol": gdt,
                "datum_refs": datums or [],
                "modifiers": mods or [],
            }
        )

    # 1..15: Linear + symmetric tolerances (preserving exact trailing zeros!)
    for idx, (nom, tol) in enumerate(
        [
            ("120.0", "0.1"),
            ("60.00", "0.05"),
            ("18.50", "0.05"),
            ("8.00", "0.02"),
            ("12.000", "0.015"),
            ("25.400", "0.025"),
            ("100.00", "0.10"),
            ("5.50", "0.05"),
            ("42.000", "0.010"),
            ("0.500", "0.005"),
            ("75.0", "0.2"),
            ("3.250", "0.005"),
            ("250.00", "0.25"),
            ("14.00", "0.05"),
            ("9.750", "0.012"),
        ]
    ):
        sep = " \u00b1" if idx % 2 == 0 else "\u00b1"
        add(f"{nom}{sep}{tol}", "linear", nominal=nom, upper=f"+{tol}", lower=f"-{tol}")

    # 16..27: Bilateral tolerances (+upper / -lower with exact trailing zeros)
    for nom, up, lo in [
        ("45.00", "+0.02", "-0.01"),
        ("12.000", "+0.018", "-0.000"),
        ("30.00", "+0.05", "-0.00"),
        ("80.000", "+0.000", "-0.025"),
        ("16.50", "+0.10", "-0.05"),
        ("6.000", "+0.012", "-0.006"),
        ("22.00", "+0.03", "-0.01"),
        ("50.000", "+0.020", "-0.000"),
        ("10.00", "+0.00", "-0.05"),
        ("35.500", "+0.015", "-0.010"),
        ("90.00", "+0.04", "-0.02"),
        ("4.000", "+0.008", "-0.004"),
    ]:
        add(f"{nom} {up}/{lo}", "linear", nominal=nom, upper=up, lower=lo)

    # 28..35: Limit dimensions (lo - hi)
    for lo, hi in [
        ("11.985", "12.015"),
        ("24.95", "25.05"),
        ("6.000", "6.018"),
        ("9.980", "10.000"),
        ("50.00", "50.10"),
        ("3.170", "3.180"),
        ("15.950", "16.050"),
        ("100.00", "100.20"),
    ]:
        add(f"{lo} - {hi}", "linear", nominal=None, upper=hi, lower=lo)

    # 36..49: Diameters (Ø, ⌀, SØ) with repeat counts, symmetric, bilateral, plain
    add("\u00d812.00 \u00b10.05", "diameter", nominal="12.00", upper="+0.05", lower="-0.05")
    add("2X \u00d86.50 \u00b10.02", "diameter", nominal="6.50", upper="+0.02", lower="-0.02", repeat=2)
    add("4X \u00d88.000 +0.015/-0.000", "diameter", nominal="8.000", upper="+0.015", lower="-0.000", repeat=4)
    add("\u230020.000 +0.021/-0.000", "diameter", nominal="20.000", upper="+0.021", lower="-0.000")
    add("6X \u00d84.20 \u00b10.05", "diameter", nominal="4.20", upper="+0.05", lower="-0.05", repeat=6)
    add("\u00d832.00", "diameter", nominal="32.00")
    add("3X \u00d810.50", "diameter", nominal="10.50", repeat=3)
    add("\u00d850.000 \u00b10.010", "diameter", nominal="50.000", upper="+0.010", lower="-0.010")
    add("8X \u00d85.00 +0.05/-0.00", "diameter", nominal="5.00", upper="+0.05", lower="-0.00", repeat=8)
    add("S\u00d818.00 \u00b10.02", "spherical_diameter", nominal="18.00", upper="+0.02", lower="-0.02")
    add("S\u00d825.000 +0.010/-0.010", "spherical_diameter", nominal="25.000", upper="+0.010", lower="-0.010")
    add("2X S\u00d810.00", "spherical_diameter", nominal="10.00", repeat=2)
    add("\u00d815.980 - 16.020", "diameter", nominal=None, upper="16.020", lower="15.980")
    add("2X \u00d86.50 H7", "diameter", nominal="6.50", repeat=2)

    # 50..59: Radii (R, SR)
    add("R4.0", "radius", nominal="4.0")
    add("R2.50 \u00b10.10", "radius", nominal="2.50", upper="+0.10", lower="-0.10")
    add("4X R3.00", "radius", nominal="3.00", repeat=4)
    add("2X R6.50 +0.20/-0.00", "radius", nominal="6.50", upper="+0.20", lower="-0.00", repeat=2)
    add("R12.000 \u00b10.025", "radius", nominal="12.000", upper="+0.025", lower="-0.025")
    add("R0.50", "radius", nominal="0.50")
    add("SR15.00 \u00b10.05", "spherical_radius", nominal="15.00", upper="+0.05", lower="-0.05")
    add("SR8.000", "spherical_radius", nominal="8.000")
    add("8X R1.50 \u00b10.10", "radius", nominal="1.50", upper="+0.10", lower="-0.10", repeat=8)
    add("R25.0", "radius", nominal="25.0")

    # 60..67: Angles & Chamfers
    add("45\u00b0 \u00b10.5\u00b0", "angle", nominal="45", upper="+0.5", lower="-0.5")
    add("90.0\u00b0 \u00b10.1\u00b0", "angle", nominal="90.0", upper="+0.1", lower="-0.1")
    add("30\u00b0 +0.5\u00b0/-0.0\u00b0", "angle", nominal="30", upper="+0.5", lower="-0.0")
    add("60.00\u00b0", "angle", nominal="60.00")
    add("2x45\u00b0", "chamfer", nominal="2")
    add("1.5x45\u00b0", "chamfer", nominal="1.5")
    add("C2.0", "chamfer", nominal="2.0")
    add("4X C1.0", "chamfer", nominal="1.0", repeat=4)

    # 68..75: Basic [X], Reference (X), Threads, Surface Finish
    add("[25.00]", "basic_dimension", nominal="25.00")
    add("[40.000]", "basic_dimension", nominal="40.000")
    add("(10.0)", "reference_dimension", nominal="10.0")
    add("(125.00)", "reference_dimension", nominal="125.00")
    add("M6x1.0-6H", "thread", nominal="6")
    add("4X M8x1.25-6H", "thread", nominal="8", repeat=4)
    add("Ra 1.6", "surface_finish", nominal="1.6")
    add("Ra 0.80", "surface_finish", nominal="0.80")

    # 76..80: Plain linear dimensions (no explicit tolerance)
    for nom in ["30.00", "55.000", "140.0", "7.50", "2X 15.00"]:
        if nom.startswith("2X "):
            add(nom, "linear", nominal="15.00", repeat=2)
        else:
            add(nom, "linear", nominal=nom)

    # 81..100: All GD&T Feature Control Frames (20 cases covering all 14 symbols + modifiers + datums)
    fcf_cases = [
        ("\u2316 | \u00d80.05 M | A | B | C", "position", "0.05", ["A", "B", "C"], ["mmc"]),
        ("\u2316 | \u00d80.10 \u24c2 | A | B", "position", "0.10", ["A", "B"], ["mmc"]),
        ("\u2316 | \u00d80.025 \u24c1 | A | B | C", "position", "0.025", ["A", "B", "C"], ["lmc"]),
        ("\u2316 | 0.050 | A | B", "position", "0.050", ["A", "B"], []),
        ("\u22a5 | 0.02 | A", "perpendicularity", "0.02", ["A"], []),
        ("\u27c2 | \u00d80.03 M | A", "perpendicularity", "0.03", ["A"], ["mmc"]),
        ("\u23e5 | 0.01", "flatness", "0.01", [], []),
        ("\u23e5 | 0.005", "flatness", "0.005", [], []),
        ("\u25cb | 0.008", "circularity", "0.008", [], []),
        ("\u232d | 0.010", "cylindricity", "0.010", [], []),
        ("\u2225 | 0.025 | A", "parallelism", "0.025", ["A"], []),
        ("\u2225 | 0.050 | A | B", "parallelism", "0.050", ["A", "B"], []),
        ("\u2220 | 0.04 | A", "angularity", "0.04", ["A"], []),
        ("\u2313 | 0.05 | A | B | C", "profile_surface", "0.05", ["A", "B", "C"], []),
        ("\u2312 | 0.02 | A | B", "profile_line", "0.02", ["A", "B"], []),
        ("\u2197 | 0.015 | A", "circular_runout", "0.015", ["A"], []),
        ("\u2330 | 0.030 | A-B", "total_runout", "0.030", ["A-B"], []),
        ("\u25ce | \u00d80.02 | A", "concentricity", "0.02", ["A"], []),
        ("\u232f | 0.04 | A", "symmetry", "0.04", ["A"], []),
        ("\u23e4 | 0.012", "straightness", "0.012", [], []),
    ]
    for raw, sym, tol, datums, mods in fcf_cases:
        add(raw, "gdt_fcf", nominal=tol, gdt=sym, datums=datums, mods=mods)

    assert len(cases) == 100, f"expected 100 cases, got {len(cases)}"

    distractors = [
        "DRAWN BY: J. SMITH",
        "CHECKED BY: A. PATEL",
        "SCALE 1:1",
        "REV C",
        "SHEET 1 OF 2",
        "UNLESS OTHERWISE SPECIFIED",
        "ALL DIMENSIONS IN MILLIMETERS",
        "THIRD ANGLE PROJECTION",
        "MATERIAL: 6061-T6 ALUMINUM",
        "DO NOT SCALE DRAWING",
        "DATE: 2026-11-22",
        "PROPRIETARY AND CONFIDENTIAL",
    ]

    return {"version": 1, "callout_count": len(cases), "callouts": cases, "distractors": distractors}


def main() -> int:
    check = "--check" in sys.argv[1:]
    want = (json.dumps(build_corpus(), indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")
    path = HERE / "callouts_corpus.json"
    if check:
        if not path.exists() or path.read_bytes() != want:
            print(f"DRIFT: {path}", file=sys.stderr)
            return 1
    else:
        path.write_bytes(want)
    print("rpt-balloon-auto callouts corpus ok (100 callouts + 12 distractors)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
