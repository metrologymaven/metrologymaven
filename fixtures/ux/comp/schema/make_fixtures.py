#!/usr/bin/env python3
"""Generate and verify the synthetic fixtures of spec node UX.COMP.SCHEMA (fid-design comp-check).

License: CC-BY-4.0 (synthetic fixtures).

Writes, below this directory:
- ok/button.md: a complete component-spec@1 file (AC-1);
- pointer-only.md, bad-token.md, no-reason.md, bad-role.md: one defect each (AC-2 .. AC-5);
- extra/*.md: one defect each for the other rules (state, focus, density, schema);
- v1.0.0/: the package fixture. It is fixtures/ux/system/pkg/v1.0.0 copied byte for byte, plus
  what a focus check needs and that package lacks: tokens/pairs.json (fid-design-pairs@1), a
  surface colour (prim.color.grey.50, sem.surface.base) and a second spacing step (space.1),
  with names.json updated to list them. The source package itself declares no pairs, so
  REQ-UX.COMP.SCHEMA-004 cannot pass against it (see the `system-pkg` case below);
- expected.json: the findings (code and subject) each case must produce, hand-labelled here
  from the rules of the spec node, never from the Rust output.

Every defect case is the AC-1 spec with one targeted edit, so it has exactly one finding.

Usage:
    python fixtures/ux/comp/schema/make_fixtures.py          # write
    python fixtures/ux/comp/schema/make_fixtures.py --check  # verify, exit 1 on drift
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[3]
SOURCE_PKG = REPO / "fixtures" / "ux" / "system" / "pkg" / "v1.0.0"

BUTTON_TOML = """\
format = "component-spec@1"
id = "comp.button"
title = "Button"

[[parts]]
name = "container"
actionable = true
description = "The area that takes the click and the keyboard focus."

[[parts]]
name = "label"

[[parts]]
name = "reason"
description = "Visible text that says why the button is disabled."

[states]
default = { applies = true }
hover = { applies = true }
focus = { applies = true }
active = { applies = true }
disabled = { applies = true }
read-only = { applies = false, reason = "A button holds no value that could be read-only." }
error = { applies = false, reason = "A button validates nothing; the action reports its own errors." }
loading = { applies = true }

[tokens.container.default]
background = "sem.surface.base"
border = "sem.text.muted"

[tokens.container.hover]
border = "sem.focus.ring"

[tokens.container.focus]
outline = "sem.focus.ring"

[tokens.label.default]
text = "sem.text.muted"

[tokens.reason.default]
text = "sem.text.muted"

[density.container.compact]
padding = "space.1"
gap = "space.1"

[density.container.comfortable]
padding = "space.2"
gap = "space.2"

[[keyboard]]
key = "Enter"
context = "The button has focus."
action = "activate"

[[keyboard]]
key = "Space"
context = "The button has focus."
action = "activate"

[[pointer_actions]]
gesture = "click"
part = "container"
action = "activate"

[a11y]
role = "button"
name_source = "The text of the label part."
states = ["busy", "disabled"]

[focus]
token = "sem.focus.ring"
against = ["sem.surface.base"]
width_px = 2
offset_px = 2

[disabled_reason]
shown_as = "visible"
part = "reason"
"""

PROSE = """\

# Button (synthetic fixture)

## Anatomy

A container holding a label, and a reason line shown while the button is disabled.

## Behaviour

Activating the button runs its action once. While the action runs the button shows loading.

## Keyboard

Enter and Space activate the focused button.

## Accessibility

Role button; the label text is the accessible name.

## Do and don't

Do keep the label short. Don't hide the disabled reason behind a hover.
"""


def spec(toml: str, prose: str = PROSE) -> str:
    return "```toml\n" + toml + "```\n" + prose


def edit(text: str, old: str, new: str) -> str:
    """Replace `old` (which must occur exactly once) by `new`."""
    if text.count(old) != 1:
        raise SystemExit(f"fixture edit: {old!r} occurs {text.count(old)} times")
    return text.replace(old, new)


def cases() -> tuple[dict[str, str], list[dict]]:
    """Spec files (path -> text) and the hand-labelled expectations."""
    files: dict[str, str] = {}
    expect: list[dict] = []

    def add(path: str, text: str, findings: list[list[str]], package: str = "v1.0.0") -> None:
        files[path] = text
        expect.append({"root": path, "package": package, "findings": findings})

    ok = spec(BUTTON_TOML)
    # AC-1: every rule holds.
    add("ok/button.md", ok, [])
    # AC-2: drag to reorder with no keyboard entry for `reorder`.
    pointer = edit(BUTTON_TOML, 'id = "comp.button"\ntitle = "Button"', 'id = "comp.sortable-row"\ntitle = "Sortable row"')
    pointer = edit(
        pointer,
        '[[pointer_actions]]\ngesture = "click"\npart = "container"\naction = "activate"\n',
        '[[pointer_actions]]\ngesture = "click"\npart = "container"\naction = "activate"\n\n'
        '[[pointer_actions]]\ngesture = "drag"\npart = "container"\naction = "reorder"\n',
    )
    add("pointer-only.md", spec(pointer), [["COMP-POINTER-ONLY", "reorder"]])
    # AC-3: a token the package does not define.
    bad_token = edit(BUTTON_TOML, '[tokens.label.default]\ntext = "sem.text.muted"', '[tokens.label.default]\ntext = "sem.text.nonexistent"')
    add("bad-token.md", spec(bad_token), [["COMP-TOKEN-UNKNOWN", "sem.text.nonexistent"]])
    # AC-4: the disabled state applies and no disabled_reason is given.
    no_reason = edit(BUTTON_TOML, '\n[disabled_reason]\nshown_as = "visible"\npart = "reason"\n', "")
    add("no-reason.md", spec(no_reason), [["COMP-DISABLED-REASON", "no-reason.md"]])
    # AC-5: a role outside the closed list.
    bad_role = edit(BUTTON_TOML, 'role = "button"', 'role = "widgetish"')
    add("bad-role.md", spec(bad_role), [["COMP-ROLE", "widgetish"]])

    # Further rules, one defect each.
    no_loading = edit(BUTTON_TOML, "loading = { applies = true }\n", "")
    add("extra/state-missing.md", spec(no_loading), [["COMP-STATE-MISSING", "loading"]])
    thin = edit(BUTTON_TOML, "width_px = 2", "width_px = 1")
    add("extra/focus-thin.md", spec(thin), [["COMP-FOCUS", "focus-thin.md"]])
    undeclared = edit(BUTTON_TOML, 'against = ["sem.surface.base"]', 'against = ["sem.status.ok"]')
    add("extra/focus-undeclared.md", spec(undeclared), [["COMP-FOCUS", "sem.focus.ring/sem.status.ok"]])
    text_pair = edit(BUTTON_TOML, 'token = "sem.focus.ring"', 'token = "sem.text.muted"')
    add("extra/focus-text-pair.md", spec(text_pair), [["COMP-FOCUS", "sem.text.muted/sem.surface.base"]])
    half = edit(BUTTON_TOML, '\n[density.container.comfortable]\npadding = "space.2"\ngap = "space.2"\n', "")
    add("extra/density-half.md", spec(half), [["COMP-DENSITY", "density-half.md"]])
    hover = edit(BUTTON_TOML, 'shown_as = "visible"', 'shown_as = "hover"')
    add("extra/reason-hover.md", spec(hover), [["COMP-DISABLED-REASON", "reason-hover.md"]])
    no_front = "# Button\n\n" + spec(BUTTON_TOML)
    add("extra/no-front-block.md", no_front, [["COMP-SCHEMA", "no-front-block.md"]])
    no_section = edit(ok, "\n## Do and don't\n", "\n## Usage notes\n")
    add("extra/missing-section.md", no_section, [["COMP-SCHEMA", "missing-section.md"]])

    # The AC-1 spec against the unmodified system package: it has neither the surface token
    # (nor space.1) nor tokens/pairs.json, so the focus rule cannot pass (the reason for v1.0.0/ here).
    expect.append(
        {
            "root": "ok/button.md",
            "package": "../../system/pkg/v1.0.0",
            "findings": [
                ["COMP-FOCUS", "sem.focus.ring/sem.surface.base"],
                ["COMP-TOKEN-UNKNOWN", "sem.surface.base"],
                ["COMP-TOKEN-UNKNOWN", "space.1"],
            ],
        }
    )
    return files, expect


def pretty(obj: object) -> str:
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def package_files() -> dict[str, bytes]:
    """The v1.0.0 package fixture: the system package plus the focus-check additions."""
    out: dict[str, bytes] = {}
    for p in sorted(SOURCE_PKG.rglob("*")):
        if p.is_file():
            out[p.relative_to(SOURCE_PKG).as_posix()] = p.read_bytes()

    prim = json.loads(out["tokens/primitive/color.tokens.json"])
    prim["prim"]["color"]["grey"]["50"] = {"$value": "#f7f8f9"}
    out["tokens/primitive/color.tokens.json"] = pretty(prim).encode()

    sem = json.loads(out["tokens/semantic/color.tokens.json"])
    sem["sem"]["surface"] = {"base": {"$value": "{prim.color.grey.50}", "$description": "Page surface."}}
    out["tokens/semantic/color.tokens.json"] = pretty(sem).encode()

    space = json.loads(out["tokens/space.tokens.json"])
    space["space"]["1"] = {"$value": "4px"}
    out["tokens/space.tokens.json"] = pretty(space).encode()

    names = json.loads(out["names.json"])
    for n in ["prim.color.grey.50", "sem.surface.base", "space.1"]:
        names["names"].append({"name": n, "kind": "token"})
    names["names"].sort(key=lambda e: e["name"])
    out["names.json"] = pretty(names).encode()

    pairs = {
        "format": "fid-design-pairs@1",
        "pairs": [
            {"fg": "sem.focus.ring", "bg": "sem.surface.base", "tag": "non-text"},
            {"fg": "sem.text.muted", "bg": "sem.surface.base", "tag": "text"},
        ],
    }
    out["tokens/pairs.json"] = pretty(pairs).encode()
    return {f"v1.0.0/{k}": v for k, v in out.items()}


def all_files() -> dict[str, bytes]:
    specs, expect = cases()
    files = {k: v.encode() for k, v in specs.items()}
    files.update(package_files())
    files["expected.json"] = pretty({"format": "comp-check-cases@1", "cases": expect}).encode()
    return files


def main() -> int:
    files = all_files()
    if "--check" in sys.argv[1:]:
        errors = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file():
                errors.append(f"missing fixture file: {rel}")
            elif p.read_bytes() != want:
                errors.append(f"fixture drift: {rel}")
        # No stray files in the generated folders.
        for d in ["ok", "extra", "v1.0.0"]:
            for p in (HERE / d).rglob("*"):
                rel = p.relative_to(HERE).as_posix()
                if p.is_file() and rel not in files:
                    errors.append(f"unexpected file: {rel}")
        for e in errors:
            print(e, file=sys.stderr)
        return 1 if errors else 0
    for rel, content in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
