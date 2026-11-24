#!/usr/bin/env python3
"""Generate and verify synthetic fixtures for OUTR.BRAND.GUIDE.DRESS.

License: CC-BY-4.0 (fictional product names for AC-2, AC-3, AC-4 of tests/trade_dress.rs).

Usage:
    python3 fixtures/outr/brand/dress/make_fixtures.py
    python3 fixtures/outr/brand/dress/make_fixtures.py --check
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def entry_block(
    name: str,
    colours: str,
    icon_style: str,
    layout: str,
    instead: str,
    reviewed_by: str = "",
    reviewed_on: str = "",
) -> str:
    return (
        "[[product]]\n"
        f'name = "{name}"\n'
        f'colours = "{colours}"\n'
        f'icon_style = "{icon_style}"\n'
        f'layout = "{layout}"\n'
        f'instead = "{instead}"\n'
        f'reviewed_by = "{reviewed_by}"\n'
        f'reviewed_on = "{reviewed_on}"\n'
    )


def build_files() -> dict[str, str]:
    incumbents = (
        "# Fictional incumbent names for OUTR.BRAND.GUIDE.DRESS fixtures (CC-BY-4.0).\n"
        "Acorn Metro\n"
        "Beryl Scan\n"
        "Cinder Probe\n"
    )

    e_acorn = entry_block(
        name="Acorn Metro",
        colours="High-saturation amber header bars on charcoal panels.",
        icon_style="Filled duotone icons combining grey and bright amber.",
        layout="Left feature tree, top ribbon strip and right property dock.",
        instead="Use neutral surface.base and surface.panel tokens with muted accent.primary.",
        reviewed_by="maintainer-a",
        reviewed_on="2026-11-20",
    )
    e_beryl = entry_block(
        name="Beryl Scan",
        colours="Vivid emerald selection outlines over dark slate panels.",
        icon_style="Thin outlined icons with emerald accent dots.",
        layout="Top workflow tabs with a split viewport and bottom stage bar.",
        instead="Use locked status.* tokens and monochrome 16/24 px icons on the 0.5 px grid.",
    )
    e_cinder = entry_block(
        name="Cinder Probe",
        colours="Classic light-grey desktop chrome with crimson status bars.",
        icon_style="Shaded bitmap-style toolbar glyphs with primary-colour cues.",
        layout="Multi-document workspace with floating toolbars and code pane.",
        instead="Use {{FAMILY_NAME}} tokenised surfaces and greyscale-safe status.* chips.",
    )

    ok_toml = "# Fictional trade-dress list (CC-BY-4.0).\n\n" + "\n".join([e_acorn, e_beryl, e_cinder])

    # AC-2: missing Cinder Probe.
    missing_toml = "# Missing Cinder Probe (AC-2, CC-BY-4.0).\n\n" + "\n".join([e_acorn, e_beryl])

    # AC-3a: layout field of exactly 400 characters on Acorn Metro.
    long_layout = ("Left explorer and tabbed toolbar layout. " * 10)[:400]
    assert len(long_layout) == 400
    e_acorn_long = entry_block(
        name="Acorn Metro",
        colours="High-saturation amber header bars on charcoal panels.",
        icon_style="Filled duotone icons combining grey and bright amber.",
        layout=long_layout,
        instead="Use neutral surface.base and surface.panel tokens with muted accent.primary.",
    )
    long_toml = "# Layout field of 400 characters (AC-3, CC-BY-4.0).\n\n" + "\n".join([e_acorn_long, e_beryl, e_cinder])

    # AC-3b: image link in colours field on Beryl Scan.
    e_beryl_img = entry_block(
        name="Beryl Scan",
        colours="Vivid emerald palette shown in ![example](beryl-header.png).",
        icon_style="Thin outlined icons with emerald accent dots.",
        layout="Top workflow tabs with a split viewport and bottom stage bar.",
        instead="Use locked status.* tokens and monochrome 16/24 px icons.",
    )
    image_toml = "# Field containing an image link (AC-3, CC-BY-4.0).\n\n" + "\n".join([e_acorn, e_beryl_img, e_cinder])

    return {
        "incumbents.txt": incumbents,
        "ok.toml": ok_toml,
        "missing.toml": missing_toml,
        "long.toml": long_toml,
        "image.toml": image_toml,
    }


def main() -> int:
    files = build_files()
    if "--check" in sys.argv[1:]:
        errors: list[str] = []
        for rel, want in sorted(files.items()):
            p = ROOT / rel
            if not p.is_file():
                errors.append(f"missing fixture file: {rel}")
                continue
            if p.read_text(encoding="utf-8") != want:
                errors.append(f"fixture drift: {rel}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    for rel, content in sorted(files.items()):
        (ROOT / rel).write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
