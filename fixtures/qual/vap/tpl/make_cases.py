#!/usr/bin/env python3
"""Fixtures and golden renders for spec node QUAL.VAP.TPL (fid-qual vap::tpl).

Writes (or, with --check, regenerates in memory and compares byte for byte):

  set-ok/*.json          eight minimal templates, one per required document (AC-1)
  set-no-supq/*.json     the same without supplier-questionnaire (AC-2)
  set-no-licence/*.json  the same with the licence removed from `risk` (AC-4)
  golden/selections.json the intended-use selection each golden render uses
  golden/<id>.md         Markdown render of vap/templates/<id>.json (AC-3)
  golden/<id>.odt        ODF Text render of vap/templates/<id>.json (AC-3, AC-5)

This is an independent stdlib-only reference (D-038): it implements the rendering rules
written in the fid-qual `vap::tpl::render` and `vap::tpl::odf` module docs and in the spec
node, without running any Rust code. The ZIP container is written field by field with
`struct` and CRC-32 comes from `zlib`; the result is then re-read with `zipfile` and
`xml.etree` as a sanity check. When the `jsonschema` package is importable (CI installs it),
every template is also validated against schemas/qual/vap-template-1.schema.json.

  python fixtures/qual/vap/tpl/make_cases.py           # write
  python fixtures/qual/vap/tpl/make_cases.py --check   # fail if stale
"""

from __future__ import annotations

import io
import json
import struct
import sys
import zipfile
import zlib
from pathlib import Path
from xml.etree import ElementTree

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
TEMPLATES = ROOT / "vap" / "templates"
SCHEMA = ROOT / "schemas" / "qual" / "vap-template-1.schema.json"

LICENCE_NOTICE = (
    "This template is part of the Metrology Maven Validation Accelerator Pack and is licensed under the "
    "Creative Commons Attribution 4.0 International licence (CC-BY-4.0). You may copy, modify and reuse it, "
    "including for your own validation documents and commercially, provided you give attribution."
)
INTERPRETATION_NOTICE = (
    "Interpretation: provisional. The framework references and judgements in this template have not been "
    "reviewed by a qualified person; check each one before relying on it."
)
REQUIRED = [
    "urs",
    "risk",
    "iq-protocol",
    "oq-protocol",
    "pq-guidance",
    "checklist-17025",
    "checklist-part11-annex11",
    "supplier-questionnaire",
]
TITLES = {
    "urs": "User requirements specification",
    "risk": "Risk assessment",
    "iq-protocol": "Installation qualification protocol",
    "oq-protocol": "Operational qualification protocol",
    "pq-guidance": "Performance qualification guidance",
    "checklist-17025": "ISO/IEC 17025 records checklist",
    "checklist-part11-annex11": "Part 11 and Annex 11 checklist",
    "supplier-questionnaire": "Supplier questionnaire",
}
# Golden renders: template ID -> intended-use selection (given unsorted on purpose; the
# renderer sorts it).
GOLDEN_SELECTIONS = {
    "urs": ["transcription_reports"],
    "risk": ["scan_inspection", "prismatic_gdt"],
    "pq-guidance": [],
    "periodic-review": [],
}

MD_SPECIAL = set("\\`*_[]<>|#&")
TO_COMPLETE = "(to be completed)"
CHOOSE_ONE = "(choose one)"
SIGNATURE = "name ______, signature ______, date ______"


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate key {k!r}")
        out[k] = v
    return out


def load_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dupes)


def dump_json(obj) -> bytes:
    return (json.dumps(obj, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


# ------------------------------------------------------------------ fixture sets


def minimal_template(doc_id: str, licence: bool = True) -> dict:
    t = {
        "format": "vap-template/1",
        "id": doc_id,
        "title": TITLES[doc_id],
        "version": "0.1.0",
        "interpretation": "provisional",
    }
    if licence:
        t["licence"] = {"spdx": "CC-BY-4.0", "notice": LICENCE_NOTICE}
    t["refs"] = []
    t["sections"] = [
        {
            "id": "main",
            "heading": "Fixture section",
            "fields": [{"id": "note", "label": "Note", "kind": "text"}],
        }
    ]
    return t


def fixture_sets() -> dict[str, bytes]:
    files: dict[str, bytes] = {}
    for doc in REQUIRED:
        files[f"set-ok/{doc}.json"] = dump_json(minimal_template(doc))
        if doc != "supplier-questionnaire":
            files[f"set-no-supq/{doc}.json"] = dump_json(minimal_template(doc))
        files[f"set-no-licence/{doc}.json"] = dump_json(minimal_template(doc, licence=doc != "risk"))
    return files


# ------------------------------------------------------------------ rendering model


def matching(selection: list[str], rule: list[str] | None) -> list[str]:
    if not rule:
        return []
    return [s for s in selection if s in rule]


def blocks(t: dict, selection: list[str]) -> list[tuple]:
    """Blocks: ("h", level, text) | ("p", text) | ("l", label, value, mark, options)
    | ("t", label, headers, rows)."""
    out: list[tuple] = [
        ("h", 1, t["title"]),
        ("p", f"Document: {t['id']}, template version {t['version']}"),
        ("p", INTERPRETATION_NOTICE),
        ("p", "Pre-filled for intended uses: " + (", ".join(selection) if selection else "none")),
        ("p", LICENCE_NOTICE),
    ]
    if t["refs"]:
        out.append(("h", 2, "Framework references"))
        out.append(
            ("t", None, ["Designator", "Edition", "Clause"], [[r["designator"], r["edition"], r["clause"]] for r in t["refs"]])
        )
    for s in t["sections"]:
        out.append(("h", 2, s["heading"]))
        if "text" in s:
            out.append(("p", s["text"]))
        for f in s["fields"]:
            kind = f["kind"]
            if kind in ("text", "choice"):
                placeholder = CHOOSE_ONE if kind == "choice" else TO_COMPLETE
                m = matching(selection, f.get("prefill"))
                value = f["value"] if m else placeholder
                mark = ", ".join(m) if m else None
                options = f["options"] if kind == "choice" else None
                out.append(("l", f["label"], value, mark, options))
            elif kind == "signature":
                out.append(("l", f["label"], SIGNATURE, None, None))
            else:
                cols = f["columns"]
                marked = any(c["fill"] == "prefill" for c in cols)
                headers = [c["heading"] for c in cols] + (["Pre-filled for"] if marked else [])
                rows = []
                for r in f["rows"]:
                    m = matching(selection, r.get("prefill"))
                    cells = []
                    for c in cols:
                        show = c["fill"] == "static" or (c["fill"] == "prefill" and m)
                        cells.append(r["cells"].get(c["id"], "") if show else "")
                    if marked:
                        cells.append(", ".join(m))
                    rows.append(cells)
                out.append(("t", f["label"], headers, rows))
    return out


def md_esc(s: str) -> str:
    return "".join("\\" + ch if ch in MD_SPECIAL else ch for ch in s)


def md_row(cells: list[str]) -> str:
    return "| " + " | ".join(md_esc(c) for c in cells) + " |"


def render_md(t: dict, selection: list[str]) -> bytes:
    parts = []
    for b in blocks(t, selection):
        if b[0] == "h":
            parts.append(("#" * b[1]) + " " + md_esc(b[2]))
        elif b[0] == "p":
            parts.append(md_esc(b[1]))
        elif b[0] == "l":
            _, label, value, mark, options = b
            line = f"**{md_esc(label)}:** {md_esc(value)}"
            if mark is not None:
                line += f" _(pre-filled for: {md_esc(mark)})_"
            if options is not None:
                line += f" (options: {md_esc(' / '.join(options))})"
            parts.append(line)
        else:
            _, label, headers, rows = b
            if label is not None:
                parts.append(f"**{md_esc(label)}**")
            lines = [md_row(headers), "|" + " --- |" * len(headers)] + [md_row(r) for r in rows]
            parts.append("\n".join(lines))
    return ("\n\n".join(parts) + "\n").encode("utf-8")


def xml_esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


def xml_row(cells: list[str]) -> str:
    inner = "".join(
        f'<table:table-cell office:value-type="string"><text:p>{xml_esc(c)}</text:p></table:table-cell>' for c in cells
    )
    return f"<table:table-row>{inner}</table:table-row>"


def content_xml(t: dict, selection: list[str]) -> str:
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" '
        'xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" '
        'xmlns:table="urn:oasis:names:tc:opendocument:xmlns:table:1.0" office:version="1.3">',
        "<office:body>",
        "<office:text>",
    ]
    n = 0
    for b in blocks(t, selection):
        if b[0] == "h":
            lines.append(f'<text:h text:outline-level="{b[1]}">{xml_esc(b[2])}</text:h>')
        elif b[0] == "p":
            lines.append(f"<text:p>{xml_esc(b[1])}</text:p>")
        elif b[0] == "l":
            _, label, value, mark, options = b
            text = f"{label}: {value}"
            if mark is not None:
                text += f" (pre-filled for: {mark})"
            if options is not None:
                text += f" (options: {' / '.join(options)})"
            lines.append(f"<text:p>{xml_esc(text)}</text:p>")
        else:
            _, label, headers, rows = b
            if label is not None:
                lines.append(f"<text:p>{xml_esc(label)}</text:p>")
            n += 1
            lines.append(f'<table:table table:name="Table{n}">')
            lines.append(f'<table:table-column table:number-columns-repeated="{len(headers)}"/>')
            lines.append("<table:table-header-rows>")
            lines.append(xml_row(headers))
            lines.append("</table:table-header-rows>")
            lines.extend(xml_row(r) for r in rows)
            lines.append("</table:table>")
    lines += ["</office:text>", "</office:body>", "</office:document-content>"]
    return "\n".join(lines) + "\n"


MIMETYPE = b"application/vnd.oasis.opendocument.text"
MANIFEST = (
    '<?xml version="1.0" encoding="UTF-8"?>\n'
    '<manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3">\n'
    '<manifest:file-entry manifest:full-path="/" manifest:version="1.3" '
    'manifest:media-type="application/vnd.oasis.opendocument.text"/>\n'
    '<manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/>\n'
    "</manifest:manifest>\n"
).encode("utf-8")


def zip_stored(members: list[tuple[str, bytes]]) -> bytes:
    """ZIP with stored members, DOS date 1980-01-01 00:00, no extra fields (see odf.rs)."""
    out = bytearray()
    central = bytearray()
    for name, data in members:
        nb = name.encode("ascii")
        crc = zlib.crc32(data) & 0xFFFFFFFF
        offset = len(out)
        out += struct.pack("<IHHHHHIIIHH", 0x04034B50, 10, 0, 0, 0, 0x0021, crc, len(data), len(data), len(nb), 0)
        out += nb + data
        central += struct.pack(
            "<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 10, 0, 0, 0, 0x0021, crc, len(data), len(data), len(nb), 0, 0, 0, 0, 0, offset
        )
        central += nb
    cd_offset = len(out)
    out += central
    out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, len(members), len(members), len(central), cd_offset, 0)
    return bytes(out)


def render_odt(t: dict, selection: list[str]) -> bytes:
    return zip_stored(
        [("mimetype", MIMETYPE), ("META-INF/manifest.xml", MANIFEST), ("content.xml", content_xml(t, selection).encode("utf-8"))]
    )


def sanity_check_odt(data: bytes, label: str) -> None:
    with zipfile.ZipFile(io.BytesIO(data)) as z:
        if z.testzip() is not None:
            raise SystemExit(f"{label}: bad CRC")
        infos = z.infolist()
        if [i.filename for i in infos] != ["mimetype", "META-INF/manifest.xml", "content.xml"]:
            raise SystemExit(f"{label}: wrong member order")
        if any(i.compress_type != zipfile.ZIP_STORED for i in infos):
            raise SystemExit(f"{label}: a member is compressed")
        if z.read("mimetype") != MIMETYPE:
            raise SystemExit(f"{label}: wrong mimetype")
        ElementTree.fromstring(z.read("content.xml"))
        ElementTree.fromstring(z.read("META-INF/manifest.xml"))


# ------------------------------------------------------------------ main


def build() -> dict[str, bytes]:
    files = fixture_sets()
    files["golden/selections.json"] = dump_json(GOLDEN_SELECTIONS)
    for doc, sel in GOLDEN_SELECTIONS.items():
        t = load_json(TEMPLATES / f"{doc}.json")
        ordered = sorted(sel)
        files[f"golden/{doc}.md"] = render_md(t, ordered)
        odt = render_odt(t, ordered)
        sanity_check_odt(odt, doc)
        files[f"golden/{doc}.odt"] = odt
    return files


def schema_cross_check(files: dict[str, bytes]) -> None:
    try:
        import jsonschema  # type: ignore
    except ImportError:
        print("make_cases: jsonschema not installed; schema cross-check skipped")
        return
    schema = load_json(SCHEMA)
    jsonschema.Draft202012Validator.check_schema(schema)
    v = jsonschema.Draft202012Validator(schema)
    docs = [(p.name, load_json(p)) for p in sorted(TEMPLATES.glob("*.json"))]
    docs += [(k, json.loads(b)) for k, b in files.items() if k.startswith("set-ok/")]
    for name, doc in docs:
        errs = sorted(v.iter_errors(doc), key=lambda e: list(e.path))
        if errs:
            raise SystemExit(f"{name}: schema error {errs[0].message}")
    bad = json.loads(files["set-no-licence/risk.json"])
    if v.is_valid(bad):
        raise SystemExit("set-no-licence/risk.json unexpectedly validates")


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    if [a for a in argv[1:] if a != "--check"]:
        sys.stderr.write(f"usage: {argv[0]} [--check]\n")
        return 2
    files = build()
    schema_cross_check(files)
    stale = []
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        for d in ("set-ok", "set-no-supq", "set-no-licence", "golden"):
            for p in sorted((HERE / d).glob("*")):
                if f"{d}/{p.name}" not in files:
                    stale.append(f"{d}/{p.name} (unexpected)")
        if stale:
            sys.stderr.write("stale fixtures:\n" + "\n".join(stale) + "\n")
            return 1
        print(f"make_cases: {len(files)} fixtures up to date")
    else:
        print(f"make_cases: wrote {len(files)} fixtures")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
