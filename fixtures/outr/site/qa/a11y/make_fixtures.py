#!/usr/bin/env python3
"""Fixtures for OUTR.SITE.QA.A11Y (static accessibility gate). Licence: CC-BY-4.0.

Writes the synthetic pages, stylesheets, token and configuration files under this folder,
and the expected results under expected/. Expected violations are hand-labelled from the
spec's rules (one deliberate violation per page, at a known line); contrast ratios are
computed here, independently of the Rust crate, from the WCAG 2.2 definition of relative
luminance and contrast ratio.

Run from the repository root:   python fixtures/outr/site/qa/a11y/make_fixtures.py
"""

from __future__ import annotations

import json
from decimal import ROUND_HALF_UP, Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent

TOKENS = {
    "themes": {
        "light": {
            "surface.base": "#ffffff",
            "text.body": "#1b2027",
            "text.muted": "#5c6670",
        },
        "dark": {
            "surface.base": "#1b2027",
            "text.body": "#e8ecf0",
            "text.muted": "#7c7b80",
        },
    }
}


def lin(c: int) -> float:
    v = c / 255
    return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4


def lum(hexcol: str) -> float:
    r, g, b = (int(hexcol[i : i + 2], 16) for i in (1, 3, 5))
    return 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(b)


def ratio(a: str, b: str) -> float:
    hi, lo = sorted((lum(a), lum(b)), reverse=True)
    return (hi + 0.05) / (lo + 0.05)


def r2(x: float) -> str:
    return str(Decimal(repr(x)).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP))


def page(title: str, body: str, *, lang: str = ' lang="en"', viewport: str = "width=device-width, initial-scale=1",
         head_extra: str = "", main_open: str = '<main id="content">', main_close: str = "</main>") -> str:
    # Lines: 1 doctype, 2 html, 3 head, 4 charset, 5 viewport, 6 title, 7 /head, 8 body,
    # 9 skip link, 10 main, 11 h1, 12 first body line of the fixture.
    return (
        "<!doctype html>\n"
        f"<html{lang}>\n"
        "<head>\n"
        '<meta charset="utf-8">\n'
        f'<meta name="viewport" content="{viewport}">\n'
        f"<title>{title}</title>\n"
        f"{head_extra}</head>\n"
        "<body>\n"
        '<a class="skip" href="#content">Skip to content</a>\n'
        f"{main_open}\n"
        f"<h1>{title or 'Untitled'}</h1>\n"
        f"{body}\n"
        f"{main_close}\n"
        "</body>\n"
        "</html>\n"
    )


def write(rel: str, text: str) -> None:
    p = HERE / rel
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text, encoding="utf-8", newline="\n")


def write_json(rel: str, value: object) -> None:
    write(rel, json.dumps(value, indent=2, sort_keys=True) + "\n")


CSS_OK = (
    "body{background:var(--surface-base);color:var(--text-body);font:16px/1.5 system-ui,sans-serif}\n"
    ".lead{color:var(--text-body);font-size:20px}\n"
    "a{color:var(--text-body)}\n"
)


def clean() -> None:
    write_json("clean/tokens.json", TOKENS)
    write("clean/style.css", CSS_OK + "@media (prefers-color-scheme: dark){\n.lead{color:var(--text-body)}\n}\n")
    write("clean/index.html", page(
        "Clean home",
        '<p class="lead">Welcome.</p>\n'
        '<img src="logo.png" alt="Logo">\n'
        '<img src="rule.png" alt="">\n'
        '<svg viewBox="0 0 8 8" aria-hidden="true"><circle cx="4" cy="4" r="3"/></svg>\n'
        '<p><a href="docs/guide.html">Read the guide</a></p>',
        head_extra='<link rel="stylesheet" href="style.css">\n'))
    write("clean/about.html", page(
        "Clean about",
        "<h2>Team</h2>\n<h3>Roles</h3>\n<h2>History</h2>\n"
        '<button type="button" aria-label="Close"><svg viewBox="0 0 8 8" aria-hidden="true"></svg></button>'))
    write("clean/contact.html", page(
        "Clean contact",
        '<form action="/send">\n'
        '<label for="name">Name</label> <input id="name" type="text">\n'
        '<label>Email <input type="email"></label>\n'
        '<textarea aria-label="Message"></textarea>\n'
        '<select title="Topic"><option>General</option></select>\n'
        '<input type="hidden" name="t" value="1">\n'
        '<input type="submit" value="Send">\n'
        "</form>\n"
        "<table>\n<tr><th>Day</th><th>Hours</th></tr>\n<tr><td>Mon</td><td>9-5</td></tr>\n</table>\n"
        '<video src="clip.mp4" autoplay muted controls></video>'))
    write("clean/docs/guide.html", page(
        "Clean guide",
        '<nav aria-label="Pages"><a href="../index.html">Home</a></nav>\n'
        '<p role="note" aria-describedby="content">Note.</p>',
        lang=' lang="en-GB"'))


RULES = {
    # page: (rule, line, html)
    "a01.html": ("A01", 12, page("Rule A01", '<img src="chart.png">')),
    "a02.html": ("A02", 12, page("Rule A02", "<h3>Details</h3>")),
    "a03.html": ("A03", 12, page("Rule A03", '<input type="text" name="q">')),
    "a04.html": ("A04", 12, page("Rule A04", "<table>\n<tr><td>1</td><td>2</td></tr>\n</table>")),
    "a05.html": ("A05", 6, page(" ", "<p>Empty title.</p>").replace("<h1> </h1>", "<h1>Rule A05</h1>")),
    "a06.html": ("A06", 12, page("Rule A06", '<a href="/next"></a>')),
    "a07.html": ("A07", 2, page("Rule A07", "<p>No lang.</p>", lang="")),
    "a08.html": ("A08", 12, page("Rule A08", '<p id="content">Duplicate id.</p>')),
    "a09.html": ("A09", 8, page("Rule A09", "<p>No main.</p>", main_open='<div id="content">', main_close="</div>")),
    "a10.html": ("A10", 5, page("Rule A10", "<p>No zoom.</p>",
                                viewport="width=device-width, initial-scale=1, maximum-scale=1")),
    "a11.html": ("A11", 12, page("Rule A11", '<video src="intro.mp4" autoplay></video>')),
}


def rules() -> None:
    write_json("rules/tokens.json", TOKENS)
    expected = []
    for name, (rule, line, text) in RULES.items():
        write(f"rules/{name}", text)
        expected.append({"page": name, "line": line, "rule": rule})
    write_json("expected/rules.json", {"exit": 1, "violations": expected})


def low_contrast() -> None:
    write_json("low-contrast/tokens.json", TOKENS)
    write("low-contrast/index.html", page("Low contrast", '<p class="note">Muted.</p>',
                                          head_extra='<link rel="stylesheet" href="style.css">\n'))
    write("low-contrast/style.css",
          "body{background:var(--surface-base);color:var(--text-body)}\n"
          ".note{color:var(--text-muted)}\n"
          ".big{color:var(--text-muted);font-size:26px}\n")
    t = TOKENS["themes"]
    dark = ratio(t["dark"]["text.muted"], t["dark"]["surface.base"])
    light = ratio(t["light"]["text.muted"], t["light"]["surface.base"])
    assert 3.0 <= dark < 4.5, dark
    assert light >= 4.5, light
    for theme in ("light", "dark"):
        assert ratio(t[theme]["text.body"], t[theme]["surface.base"]) >= 4.5
    write_json("expected/low-contrast.json", {
        "exit": 1,
        "violations": [{"page": "style.css", "line": 2, "rule": "A12", "element": ".note", "theme": "dark",
                        "ratio": r2(dark)}],
        "light_ratio": r2(light),
    })


def inline_colour() -> None:
    write_json("inline-colour/tokens.json", TOKENS)
    write("inline-colour/index.html", page("Inline colour", '<p style="color:#777">Grey.</p>'))
    write_json("expected/inline-colour.json",
               {"exit": 1, "violations": [{"page": "index.html", "line": 12, "rule": "A13"}]})


def suppressed() -> None:
    write_json("suppressed/tokens.json", TOKENS)
    write("suppressed/layout.html", page("Layout table", "<table>\n<tr><td>a</td><td>b</td></tr>\n</table>"))
    write_json("suppressed/config.json", {"suppressions": [
        {"page": "layout.html", "rule": "A04", "reason": "Legacy layout grid kept until the page is rebuilt."},
        {"page": "gone.html", "rule": "A01", "reason": "Page was removed; entry is stale."},
    ]})
    write_json("expected/suppressed.json", {
        "exit": 0,
        "suppressed": [{"page": "layout.html", "line": 12, "rule": "A04"}],
        "used": [{"page": "layout.html", "rule": "A04"}],
        "unmatched": [{"page": "gone.html", "rule": "A01"}],
    })


def bad_tokens() -> None:
    write("bad-tokens/tokens.json", '{"themes": {"light": {"text.body": "#1b')
    write("bad-tokens/index.html", page("Bad tokens", "<p>Fine page.</p>"))
    write_json("expected/bad-tokens.json", {"exit": 2, "code": "A11Y-PARSE", "file_suffix": "tokens.json"})


if __name__ == "__main__":
    clean()
    rules()
    low_contrast()
    inline_colour()
    suppressed()
    bad_tokens()
