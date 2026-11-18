"""Writes the synthetic fixtures of spec node OUTR.BRAND.ASSET.ATTR (CC-BY-4.0).

Every file here is made up for the test. No third-party image, font or logo is included:
- the PNG files are 1x1 pixel images built below with zlib and struct;
- the SVG files are a single project-drawn rectangle;
- the "fonts" are stub files with a font-like magic number followed by made-up metadata text;
  they are not usable fonts and contain no glyphs from any real typeface.
"Examplo Sans", "Synthetic Foundry" and "Synthetic Photographer" are invented names, and the
example.com / example.net / example.org hosts are the IANA reserved example domains. The rows
that call a file "third-party" do so only to exercise the check.

Expected results are hand-labelled from the spec's rules in EXPECTED below and asserted by
crates/fid-outreach/tests/asset_check.rs; nothing here was produced by running the Rust code.

Run from the repository root: python fixtures/outr/brand/attr/make_fixtures.py
"""

import os
import shutil
import struct
import zlib

HERE = os.path.dirname(os.path.abspath(__file__))
CASES = ["ok", "missing", "nc", "remote-font", "orphan", "incomplete", "html-remote", "evasion"]


def png_1px(rgb):
    """A valid 1x1 RGB PNG of colour rgb."""

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    ihdr = struct.pack(">IIBBBBB", 1, 1, 8, 2, 0, 0, 0)
    raw = b"\x00" + bytes(rgb)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", ihdr) + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")


SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8" viewBox="0 0 8 8">'
    '<title>Synthetic fixture mark</title><rect width="8" height="8" fill="#336699"/></svg>\n'
)


def font_stub(magic, family):
    """Not a font: a magic number followed by made-up metadata, for the test only."""
    meta = (
        "family=%s; foundry=Synthetic Foundry; licence=OFL-1.1; "
        "note=synthetic test stub, contains no glyphs\n" % family
    )
    return magic + b"\x00\x00\x00\x00" + meta.encode("ascii")


HEAD = "# Attribution\n\nSynthetic fixture for OUTR.BRAND.ASSET.ATTR. Every asset below is made up.\n\n"
TABLE = "| path | origin | source | author | licence |\n|---|---|---|---|---|\n"
ORIGINAL_SVG = "| `img/originals/*.svg` | original | - | project contributors | CC-BY-4.0 |\n"
LOGO_ROW = "| img/logo.svg | original | - | project contributors | CC-BY-4.0 |\n"

FILES = {
    "ok": {
        "ATTRIBUTION.md": HEAD
        + TABLE
        + ORIGINAL_SVG
        + "| img/photo.png | third-party | https://example.com/synthetic/photo | Synthetic Photographer | CC0-1.0 |\n"
        + "| fonts/examplo-sans.woff2 | third-party | https://example.com/synthetic/examplo-sans "
        "| Synthetic Foundry | OFL-1.1 |\n"
        + "\nText after the table is ignored.\n",
        "img/originals/mark.svg": SVG,
        "img/originals/mark-dark.svg": SVG.replace("#336699", "#99ccff"),
        "img/photo.png": png_1px((10, 120, 200)),
        "fonts/examplo-sans.woff2": font_stub(b"wOF2", "Examplo Sans"),
        "notes.txt": "Synthetic fixture; text files are not assets.\n",
        "css/site.css": (
            "/* Synthetic fixture: every font and stylesheet is on the site origin. */\n"
            '@import url("/css/base.css");\n'
            '@import "https://example.org/css/print.css";\n'
            "@font-face {\n"
            '  font-family: "Examplo Sans";\n'
            '  src: url(../fonts/examplo-sans.woff2) format("woff2"),\n'
            '       url("https://example.org:443/fonts/examplo-sans.woff2") format("woff2");\n'
            "}\n"
            "/* @import url(https://fonts.example.net/commented-out.css); */\n"
            'body { font-family: "Examplo Sans", sans-serif; }\n'
        ),
        "index.html": (
            "<!doctype html>\n"
            '<html lang="en">\n'
            "<head>\n"
            '<link rel="stylesheet" href="/css/site.css">\n'
            '<link rel="preload" href="https://EXAMPLE.org/fonts/examplo-sans.woff2" as="font" crossorigin>\n'
            '<link rel="icon" href="img/originals/mark.svg">\n'
            '<!-- <link rel="stylesheet" href="https://fonts.example.net/css"> -->\n'
            "<style>@font-face { font-family: X; src: url(data:font/woff2;base64,AAAA); }</style>\n"
            "</head>\n"
            "<body><p>Synthetic fixture page.</p></body>\n"
            "</html>\n"
        ),
    },
    "missing": {
        "ATTRIBUTION.md": HEAD + TABLE + "| img/*.svg | original | - | project contributors | CC-BY-4.0 |\n",
        "img/logo.svg": SVG,
        "img/x.png": png_1px((200, 10, 10)),
    },
    "nc": {
        "ATTRIBUTION.md": HEAD
        + TABLE
        + LOGO_ROW
        + "| img/photo.png | third-party | https://example.com/synthetic/nc-photo | Synthetic Photographer "
        "| CC-BY-NC-4.0 |\n",
        "img/logo.svg": SVG,
        "img/photo.png": png_1px((10, 200, 10)),
    },
    "remote-font": {
        "ATTRIBUTION.md": HEAD + TABLE + LOGO_ROW,
        "img/logo.svg": SVG,
        # Line 3 holds the remote font source.
        "css/fonts.css": (
            "/* Synthetic fixture: a font loaded from another origin. */\n"
            "\n"
            "@font-face { src: url(https://fonts.example.net/a.woff2) }\n"
        ),
    },
    "orphan": {
        "ATTRIBUTION.md": HEAD
        + TABLE
        + LOGO_ROW
        + "| img/deleted.png | third-party | https://example.com/synthetic/deleted | Synthetic Photographer "
        "| CC0-1.0 |\n",
        "img/logo.svg": SVG,
    },
    "incomplete": {
        "ATTRIBUTION.md": HEAD
        + TABLE
        + "| img/logo.svg | original | - | A. Person | CC-BY-4.0 |\n"
        + "| img/photo.png | third-party | - |  | CC0-1.0 |\n"
        + "| fonts/stub.otf | third-party | https://example.com/synthetic/stub | Synthetic Foundry "
        "| OFL-1.1 OR GPL-3.0-only |\n",
        "img/logo.svg": SVG,
        "img/photo.png": png_1px((1, 2, 3)),
        "fonts/stub.otf": font_stub(b"OTTO", "Examplo Stub"),
    },
    "html-remote": {
        "ATTRIBUTION.md": HEAD + TABLE + LOGO_ROW,
        "img/logo.svg": SVG,
        "index.html": (
            "<!doctype html>\n"
            "<html><head>\n"
            '<link rel="stylesheet" href="/site.css">\n'
            "<LINK href='https://fonts.example.net/css2?family=Examplo' REL='Stylesheet'>\n"
            '<link rel="preload" as="font" href="//cdn.example.net/f.woff2">\n'
            "<style>\n"
            "  @import url('http://example.org/insecure.css');\n"
            "</style>\n"
            "</head><body></body></html>\n"
        ),
    },
}

# Obfuscated remote sources (review of PR #115). Each line is listed with its 1-based number in
# the comment; "\t" in EVASION_HTML line 6 is a real tab character.
EVASION_HTML = "".join(
    [
        "<!doctype html>\n",  # 1
        "<html><head>\n",  # 2
        '<base href="https://cdn.example.net/">\n',  # 3: remote base
        '<link rel=stylesheet href="' + "\\\\" + 'fonts.example.net/a.css">\n',  # 4: \\host
        '<link rel=stylesheet href="/' + "\\" + 'fonts.example.net/b.css">\n',  # 5: /\host
        '<link rel=stylesheet href="ht\ttps://fonts.example.net/c.css">\n',  # 6: tab in scheme
        '<link rel=stylesheet href="https://fonts.example.net' + "\\" + '@example.org/d.css">\n',  # 7: \@
        '<link rel=stylesheet href="https&#58;//fonts.example.net/e.css">\n',  # 8: numeric reference
        '<link rel=stylesheet href="https&colon;//fonts.example.net/f.css">\n',  # 9: named reference
        '<link rel="style&#115;heet" href="//fonts.example.net/g.css">\n',  # 10: encoded rel
        '<link rel=stylesheet href="/ok.css">\n',  # 11: same origin, no finding
        "</head><body></body></html>\n",  # 12
    ]
)

EVASION_CSS = "".join(
    [
        "/* Synthetic fixture: escaped and obfuscated remote sources. */\n",  # 1
        '@import "' + "\\" + '68 ttps://fonts.example.net/i.css";\n',  # 2: \68 = h
        "@" + "\\" + "66ont-face { src: u" + "\\" + "72l(https://fonts.example.net/j.woff2) }\n",  # 3
        '@font-face { font-family: "}"; src: url(https://fonts.example.net/k.woff2) }\n',  # 4
        "@import url(https" + "\\" + "3a //fonts.example.net/l.css);\n",  # 5: \3a = :
        '@font-face { src: url("/fonts/local.woff2") }\n',  # 6: same origin, no finding
    ]
)

EVASION_SVG = "".join(
    [
        '<?xml version="1.0"?>\n',  # 1
        '<?xml-stylesheet type="text/css" href="https://fonts.example.net/n.css"?>\n',  # 2
        '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">\n',  # 3
        "<style><![CDATA[\n",  # 4
        "@import url(https://fonts.example.net/m.css);\n",  # 5
        "]]></style>\n",  # 6
        '<rect width="8" height="8" fill="#336699"/></svg>\n',  # 7
    ]
)

FILES["evasion"] = {
    "ATTRIBUTION.md": HEAD + TABLE + "| img/*.svg | original | - | project contributors | CC-BY-4.0 |\n",
    "index.html": EVASION_HTML,
    "css/x.css": EVASION_CSS,
    "img/logo.svg": EVASION_SVG,
}

# Hand-labelled from the spec's rules (REQ-002 .. REQ-006, Design) with --origin
# https://example.org. Text-mode finding lines, in report order; exit code.
EXPECTED = {
    "ok": (0, []),
    "missing": (1, ["ATTR-MISSING img/x.png"]),
    "nc": (1, ["ATTR-LICENCE img/photo.png CC-BY-NC-4.0"]),
    "remote-font": (1, ["FONT-REMOTE css/fonts.css:3 https://fonts.example.net/a.woff2"]),
    "orphan": (1, ["ATTR-ORPHAN img/deleted.png"]),
    "incomplete": (
        1,
        [
            "ATTR-INCOMPLETE img/logo.svg author must be `project contributors`",
            "ATTR-INCOMPLETE img/photo.png missing source, author",
            "ATTR-LICENCE fonts/stub.otf OFL-1.1 OR GPL-3.0-only",
        ],
    ),
    # http://example.org is another origin than https://example.org (scheme and port differ).
    "html-remote": (
        1,
        [
            "FONT-REMOTE index.html:4 https://fonts.example.net/css2?family=Examplo",
            "FONT-REMOTE index.html:5 //cdn.example.net/f.woff2",
            "FONT-REMOTE index.html:7 http://example.org/insecure.css",
        ],
    ),
    # Details are the URL after decoding and browser normalisation (tab removed, \ read as /).
    "evasion": (
        1,
        [
            "FONT-REMOTE css/x.css:2 https://fonts.example.net/i.css",
            "FONT-REMOTE css/x.css:3 https://fonts.example.net/j.woff2",
            "FONT-REMOTE css/x.css:4 https://fonts.example.net/k.woff2",
            "FONT-REMOTE css/x.css:5 https://fonts.example.net/l.css",
            "FONT-REMOTE img/logo.svg:2 https://fonts.example.net/n.css",
            "FONT-REMOTE img/logo.svg:5 https://fonts.example.net/m.css",
            "FONT-REMOTE index.html:3 https://cdn.example.net/",
            "FONT-REMOTE index.html:4 //fonts.example.net/a.css",
            "FONT-REMOTE index.html:5 //fonts.example.net/b.css",
            "FONT-REMOTE index.html:6 https://fonts.example.net/c.css",
            "FONT-REMOTE index.html:7 https://fonts.example.net/@example.org/d.css",
            "FONT-REMOTE index.html:8 https://fonts.example.net/e.css",
            "FONT-REMOTE index.html:9 https://fonts.example.net/f.css",
            "FONT-REMOTE index.html:10 //fonts.example.net/g.css",
        ],
    ),
}

ALLOW = """# Synthetic fixture allowlist with the schema of house-rules/spdx-allowlist.toml (ECO.RULES.SPDX).
licences = ["MIT", "Apache-2.0"]
exceptions = []
asset_licences = ["CC0-1.0", "CC-BY-4.0", "OFL-1.1", "MIT", "Apache-2.0"]
asset_paths = ["assets/**"]

[basis]
MIT = "project licence"
"Apache-2.0" = "project licence"
"""

ALLOW_BROKEN = """# Synthetic fixture: not valid TOML (unterminated array).
asset_licences = ["CC0-1.0", "OFL-1.1"
"""

LICENSE = """Synthetic test fixtures for spec node OUTR.BRAND.ASSET.ATTR (CC-BY-4.0).
All files are project-made by make_fixtures.py: 1x1 PNGs, a one-rectangle SVG and font stub
files with made-up metadata. No third-party image, font or logo is included; names such as
"Examplo Sans" and "Synthetic Foundry" are invented.
"""


def write(path, content):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    mode = "wb" if isinstance(content, bytes) else "w"
    kwargs = {} if isinstance(content, bytes) else {"encoding": "utf-8", "newline": "\n"}
    with open(path, mode, **kwargs) as f:
        f.write(content)


def main():
    for case in CASES:
        d = os.path.join(HERE, case)
        if os.path.isdir(d):
            shutil.rmtree(d)
        for rel, content in FILES[case].items():
            write(os.path.join(d, *rel.split("/")), content)
    write(os.path.join(HERE, "allow.toml"), ALLOW)
    write(os.path.join(HERE, "allow-broken.toml"), ALLOW_BROKEN)
    write(os.path.join(HERE, "LICENSE.txt"), LICENSE)
    assert set(EXPECTED) == set(CASES)


if __name__ == "__main__":
    main()
