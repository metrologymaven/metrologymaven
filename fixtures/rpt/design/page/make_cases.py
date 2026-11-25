#!/usr/bin/env python3
"""Independent oracle for RPT.DESIGN.PAGE (page setup of reports).

Written from the rules of spec/rpt/design/page/NODE.md with the Python standard library only
(exact rational arithmetic through fractions.Fraction); nothing here is produced by running
the Rust code. Writes, all CC-BY-4.0, invented data with placeholder titles (D-038):

  ../one_page_report/report.json    a report with one page (RPT.MODEL.DOC create_report)
  ../three_page_report/report.json  a report with three pages (create_report + 2 add_page)
  cases.json                        length conversions and command scenarios

Rules (lengths are integers in 0.01 mm "units"):

  length text   <digits>[.<digits>](mm|in); 1 mm = 100 units, 1 in = 2540 units exactly.
                Anything else -> not_a_length; not a whole number of units -> not_exact;
                more than 2000 mm -> too_large. Never rounded.
  presets       A4 21000 x 29700, A3 29700 x 42000, Letter 21590 x 27940 (portrait);
                landscape swaps width and height. custom: width and height each 5000..200000.
  body          x = left, y = top + header, width = page_w - left - right,
                height = page_h - top - bottom - header - footer; each must be >= 2000.
  defaults      a report that never had a layout: A4 portrait, margins 2000 each, header 0,
                footer 0. Parameters left out (orientation, margins, header, footer) keep the
                value of the layout in effect for the target: the page's effective layout
                (its override, else the default) for a page, the report default otherwise.
  check order   size: a preset with width or height given -> that field only_for_custom
                (width first); custom without width or height -> required_for_custom; then
                width and height parsed; then margins (not four parts -> margins
                need_four_lengths; then margin_top, margin_right, margin_bottom, margin_left
                parsed); header; footer; report unknown -> report.not_found; page not a live
                page of the report -> report.not_found; custom width then height outside
                5000..200000 -> outside_custom_range; body width < 2000 -> body_width
                body_too_narrow; body height < 2000 -> body_height body_too_short.
  journal       a successful set journals one event; a failure journals nothing; show
                journals nothing.

Page handles P1, P2, ... name the report's pages in order; "X" is a page ID the report does
not have and "R?" a report that does not exist. All integers are written as decimal strings.
"""

import json
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
DESIGN = HERE.parent
OUT_CASES = HERE / "cases.json"
OUT_ONE = DESIGN / "one_page_report" / "report.json"
OUT_THREE = DESIGN / "three_page_report" / "report.json"

UNITS = {"mm": Fraction(100), "in": Fraction(2540)}
MAX_LENGTH = 200000
PRESETS = {"A4": (21000, 29700), "A3": (29700, 42000), "Letter": (21590, 27940)}
MIN_CUSTOM, MAX_CUSTOM, MIN_BODY = 5000, 200000, 2000
DEFAULT = {
    "size": "A4",
    "size_width": 21000,
    "size_height": 29700,
    "orientation": "portrait",
    "margins": {"top": 2000, "right": 2000, "bottom": 2000, "left": 2000},
    "header": 0,
    "footer": 0,
}


class Refused(Exception):
    def __init__(self, code, field=None, reason=None):
        super().__init__(code)
        self.code, self.field, self.reason = code, field, reason


def length(field, text):
    """Units of a length text, or Refused(report.layout_invalid, field, reason)."""
    unit = text[-2:] if len(text) >= 2 else ""
    number = text[:-2]
    ok = unit in UNITS and number != ""
    if ok:
        int_part, dot, frac_part = number.partition(".")
        ok = int_part.isdigit() and int_part.isascii() and (not dot or (frac_part.isdigit() and frac_part.isascii()))
    if not ok:
        raise Refused("report.layout_invalid", field, "not_a_length")
    value = Fraction(number) * UNITS[unit]
    if value.denominator != 1:
        raise Refused("report.layout_invalid", field, "not_exact")
    if value > MAX_LENGTH:
        raise Refused("report.layout_invalid", field, "too_large")
    return int(value)


def page_dims(layout):
    w, h = layout["size_width"], layout["size_height"]
    return (h, w) if layout["orientation"] == "landscape" else (w, h)


def body(layout):
    w, h = page_dims(layout)
    m = layout["margins"]
    return {
        "x": m["left"],
        "y": m["top"] + layout["header"],
        "width": w - m["left"] - m["right"],
        "height": h - m["top"] - m["bottom"] - layout["header"] - layout["footer"],
    }


def strs(v):
    if isinstance(v, dict):
        return {k: strs(x) for k, x in v.items()}
    if isinstance(v, int) and not isinstance(v, bool):
        return str(v)
    return v


def effective(layout):
    w, h = page_dims(layout)
    out = dict(layout)
    out["page_width"], out["page_height"], out["body"] = w, h, body(layout)
    return strs(out)


class Report:
    def __init__(self, pages):
        self.pages = [f"P{i}" for i in range(1, pages + 1)]
        self.default = dict(DEFAULT)
        self.overrides = {}

    def effective(self, page):
        return self.overrides.get(page, self.default)

    def set(self, p):
        size = p["size"]
        if size in PRESETS:
            for f in ("width", "height"):
                if p.get(f) is not None:
                    raise Refused("report.layout_invalid", f, "only_for_custom")
            sw, sh = PRESETS[size]
        else:
            for f in ("width", "height"):
                if p.get(f) is None:
                    raise Refused("report.layout_invalid", f, "required_for_custom")
            sw, sh = length("width", p["width"]), length("height", p["height"])
        margins = None
        if p.get("margins") is not None:
            parts = p["margins"].split(",")
            if len(parts) != 4:
                raise Refused("report.layout_invalid", "margins", "need_four_lengths")
            names = ("top", "right", "bottom", "left")
            margins = {n: length("margin_" + n, t) for n, t in zip(names, parts)}
        header = length("header", p["header"]) if p.get("header") is not None else None
        footer = length("footer", p["footer"]) if p.get("footer") is not None else None
        if p["report"] != "R":
            raise Refused("report.not_found")
        page = p.get("page")
        if page is not None and page not in self.pages:
            raise Refused("report.not_found")
        base = self.default if page is None else self.effective(page)
        layout = {
            "size": size,
            "size_width": sw,
            "size_height": sh,
            "orientation": p.get("orientation") or base["orientation"],
            "margins": dict(margins if margins is not None else base["margins"]),
            "header": header if header is not None else base["header"],
            "footer": footer if footer is not None else base["footer"],
        }
        if size == "custom":
            for f, v in (("width", sw), ("height", sh)):
                if not MIN_CUSTOM <= v <= MAX_CUSTOM:
                    raise Refused("report.layout_invalid", f, "outside_custom_range")
        b = body(layout)
        if b["width"] < MIN_BODY:
            raise Refused("report.layout_invalid", "body_width", "body_too_narrow")
        if b["height"] < MIN_BODY:
            raise Refused("report.layout_invalid", "body_height", "body_too_short")
        if page is None:
            self.default = layout
        else:
            self.overrides[page] = layout
        return layout

    def show(self):
        return [
            dict(effective(self.effective(pg)), page_id=pg, source="override" if pg in self.overrides else "default")
            for pg in self.pages
        ]


def run(name, fixture, pages, calls):
    rep = Report(pages)
    steps = []
    for call in calls:
        params = {"report": "R", **call}
        try:
            layout = rep.set(params)
            expect = {"result": dict(effective(layout), page=params.get("page"), report="R")}
            journaled = True
        except Refused as e:
            expect = {"error": e.code}
            if e.field is not None:
                expect["field"], expect["reason"] = e.field, e.reason
            journaled = False
        steps.append({"params": params, "expect": expect, "journaled": journaled, "show": rep.show()})
    return {"name": name, "fixture": fixture, "steps": steps}


ONE, THREE = "one_page_report", "three_page_report"
M15 = "15mm,15mm,15mm,15mm"

SCENARIOS = [
    # AC-1
    ("ac1_a4_landscape", ONE, 1, [
        {"size": "A4", "orientation": "landscape", "margins": M15, "header": "12mm", "footer": "10mm"},
    ]),
    # AC-2 (fresh fixture; orientation left out keeps portrait)
    ("ac2_custom_inches", ONE, 1, [{"size": "custom", "width": "8.5in", "height": "11in"}]),
    # AC-1 then AC-2 on the same report: orientation and margins are kept, the stored size
    # (size_width x size_height) is still exactly 21590 x 27940.
    ("ac1_then_ac2", ONE, 1, [
        {"size": "A4", "orientation": "landscape", "margins": M15, "header": "12mm", "footer": "10mm"},
        {"size": "custom", "width": "8.5in", "height": "11in"},
    ]),
    # AC-3
    ("ac3_inexact_margin", ONE, 1, [{"size": "A4", "margins": "10.005mm,10mm,10mm,10mm"}]),
    # AC-4
    ("ac4_body_too_narrow", ONE, 1, [{"size": "A4", "orientation": "portrait", "margins": "10mm,100mm,10mm,100mm"}]),
    # AC-5 and the data of AC-6
    ("ac5_page2_landscape", THREE, 3, [{"page": "P2", "size": "A4", "orientation": "landscape"}]),
    # More rules (REQ-001..006)
    ("presets", ONE, 1, [
        {"size": "A3", "orientation": "landscape"},
        {"size": "Letter", "orientation": "landscape"},
        {"size": "Letter", "orientation": "portrait", "margins": "0.5in,0.75in,0.5in,0.75in"},
        {"size": "A3"},
    ]),
    ("default_and_overrides", THREE, 3, [
        {"page": "P2", "size": "A4", "orientation": "landscape"},
        {"size": "Letter", "margins": "1in,1in,1in,1in", "header": "0.5in"},
        {"page": "P3", "size": "A3"},
        {"page": "P2", "size": "A4", "footer": "8mm"},
        {"size": "A4", "orientation": "landscape"},
    ]),
    ("refusals", ONE, 1, [
        {"size": "A4", "width": "100mm"},
        {"size": "Letter", "height": "100mm"},
        {"size": "custom", "height": "100mm"},
        {"size": "custom", "width": "100mm"},
        {"size": "custom", "width": "49.99mm", "height": "100mm"},
        {"size": "custom", "width": "100mm", "height": "2000.01mm"},
        {"size": "custom", "width": "0.01in", "height": "100mm"},
        {"size": "A4", "margins": "10mm,10mm,10mm"},
        {"size": "A4", "margins": "10mm,10mm,10mm,10mm,10mm"},
        {"size": "A4", "margins": "10mm,10mm,10mm,-1mm"},
        {"size": "A4", "margins": "10mm,10mm,10mm, 10mm"},
        {"size": "A4", "header": "12cm"},
        {"size": "A4", "footer": "1.5"},
        {"size": "A4", "header": "260mm"},
        {"size": "A4", "orientation": "landscape", "header": "100mm", "footer": "51mm"},
        {"size": "A4", "page": "X"},
        {"size": "A4", "report": "R?"},
        {"size": "A4", "margins": "10.001mm,10mm,10mm,10mm", "report": "R?"},
    ]),
    ("boundaries", ONE, 1, [
        {"size": "custom", "width": "50mm", "height": "2000mm", "margins": "0mm,0mm,0mm,0mm"},
        {"size": "custom", "width": "2000mm", "height": "50mm", "margins": "0mm,0mm,0mm,0mm"},
        {"size": "A4", "margins": "0mm,95mm,0mm,95mm"},
        {"size": "A4", "margins": "0mm,95mm,0mm,95.01mm"},
        {"size": "A4", "margins": "0mm,0mm,0mm,0mm", "header": "100mm", "footer": "177mm"},
        {"size": "A4", "margins": "0mm,0mm,0mm,0mm", "header": "100mm", "footer": "177.01mm"},
        {"size": "custom", "width": "49.99mm", "height": "2000mm"},
    ]),
]

LENGTHS = [
    "15mm", "0mm", "000mm", "10.25mm", "10.2500mm", "0.01mm", "8.5in", "11in", "0.5in", "0.05in",
    "0.01in", "0.25in", "1.27in", "10.005mm", "0.001mm", "2000mm", "2000.00mm", "2000.01mm",
    "78.74in", "78.75in", "99999999mm", "", "mm", "in", "1", "-1mm", "+1mm", "1.mm", ".5mm",
    "1 mm", "1cm", "1e2mm", "1,5mm", "1.5MM", "１mm", "0x10mm",
]


def length_case(text):
    try:
        return {"text": text, "units": str(length("f", text))}
    except Refused as e:
        return {"text": text, "error": e.reason}


def build():
    cases = {
        "lengths": [length_case(t) for t in LENGTHS],
        "scenarios": [run(*s) for s in SCENARIOS],
    }
    one = {"fixture": ONE, "title": "Sample part A first article report", "pages": "1"}
    three = {"fixture": THREE, "title": "Sample part B three page report", "pages": "3"}
    dump = lambda v: json.dumps(v, indent=1, sort_keys=True, ensure_ascii=True) + "\n"
    return {OUT_CASES: dump(cases), OUT_ONE: dump(one), OUT_THREE: dump(three)}


def self_check(files):
    """The acceptance criteria as the spec states them, against the oracle's output."""
    sc = {s["name"]: s["steps"] for s in json.loads(files[OUT_CASES])["scenarios"]}
    ac1 = sc["ac1_a4_landscape"][0]["show"][0]
    assert (ac1["page_width"], ac1["page_height"]) == ("29700", "21000")
    assert ac1["body"] == {"x": "1500", "y": "2700", "width": "26700", "height": "15800"}
    ac2 = sc["ac2_custom_inches"][0]["expect"]["result"]
    assert (ac2["size_width"], ac2["size_height"]) == ("21590", "27940")
    assert (ac2["page_width"], ac2["page_height"]) == ("21590", "27940")
    ac2b = sc["ac1_then_ac2"][1]["expect"]["result"]
    assert (ac2b["size_width"], ac2b["size_height"]) == ("21590", "27940")
    ac3 = sc["ac3_inexact_margin"][0]
    assert ac3["expect"] == {"error": "report.layout_invalid", "field": "margin_top", "reason": "not_exact"}
    assert not ac3["journaled"]
    ac4 = sc["ac4_body_too_narrow"][0]
    assert ac4["expect"]["error"] == "report.layout_invalid" and ac4["expect"]["field"] == "body_width"
    ac5 = sc["ac5_page2_landscape"][0]["show"]
    assert [p["orientation"] for p in ac5] == ["portrait", "landscape", "portrait"]
    assert [p["source"] for p in ac5] == ["default", "override", "default"]


def main():
    files = build()
    self_check(files)
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in files.items() if not p.exists() or p.read_text(encoding="utf-8") != text]
        for p in stale:
            print(f"{p} is stale; run {Path(__file__).name}", file=sys.stderr)
        if stale:
            return 1
        print("page setup fixtures up to date")
        return 0
    for p, text in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
