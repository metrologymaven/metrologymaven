#!/usr/bin/env python3
"""Independent oracle for RPT.DESIGN.PLACE.FRAME (block frames, move, resize, grid snapping).

Written from the rules of spec/rpt/design/place/frame/NODE.md with the Python standard library
only (exact rational arithmetic through fractions.Fraction); nothing here is produced by
running the Rust code. Writes cases.json (CC-BY-4.0, invented data with placeholder text,
D-038). The reports are the shared fixtures ../one_page_report and ../three_page_report of
RPT.DESIGN.PAGE; the blocks each scenario adds to them (text stub blocks B1, B2, ...) are
listed in the scenario itself, so the shared fixtures stay untouched.

Rules (lengths are integers in 0.01 mm "units"; a frame is x, y, w, h relative to the page
body origin):

  length text    <digits>[.<digits>](mm|in); 1 mm = 100 units, 1 in = 2540 units exactly;
                 not that shape -> not_a_length; not a whole number of units -> not_exact;
                 more than 2000 mm -> too_large. Never rounded. x, y, dx, dy, dw and dh may
                 carry one leading "-"; w and h may not.
  grid pitch     a setting in units, 10 (0.1 mm) .. 200000 (2000 mm), default 100 (1 mm);
                 typed as a length text it is parsed as above (field grid_pitch), then
                 below 10 -> below_minimum. Only read when snap is on.
  target frame   absolute: x, y, w, h each optional; delta: dx, dy, dw, dh each optional,
                 added to the stored frame. A coordinate the command leaves out keeps the
                 stored value. A block with no stored frame needs every coordinate given
                 absolutely; otherwise the first coordinate (x, y, w, h order) that needs the
                 stored frame -> <coordinate> no_frame.
  snapping       with snap on, each edge (left x, top y, right x + w, bottom y + h) is
                 rounded to the nearest multiple of the pitch, ties away from zero; then
                 w = right - left and h = bottom - top.
  bounds         after snapping: |x| and |y| at most 200000 (too_large); w and h at least
                 100 (1 mm, too_small) and at most 200000 (too_large); checked x, y, w, h.
                 A frame may lie outside the body (no body check at all).
  page           with page given, the block moves to the end of that page's block list
                 (page membership); the frame is kept relative to the body origin.
  check order    lengths x, y, w, h, dx, dy, dw, dh parsed; grid pitch (snap on only);
                 report unknown -> report.not_found; block not live in the report ->
                 report.not_found; page given and not a live page -> report.not_found; the
                 target frame (no_frame); snapping; bounds.
  journal        a successful move journals one event; a failure journals nothing.

Handles: R the report, R? a report that does not exist, P<n> the report's pages in order,
X a page ID the report does not have, B<n> the scenario's blocks, B? a block ID the report
does not have. All integers in results are written as decimal strings.
"""

import json
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_CASES = HERE / "cases.json"

UNITS = {"mm": Fraction(100), "in": Fraction(2540)}
MAX_LENGTH = 200000
MIN_SIDE = 100
MIN_PITCH, MAX_PITCH, DEFAULT_PITCH = 10, 200000, 100
PAGES = {"one_page_report": 1, "three_page_report": 3}
SIGNED = {"x", "y", "dx", "dy", "dw", "dh"}
ABSOLUTE = ["x", "y", "w", "h"]
DELTA = ["dx", "dy", "dw", "dh"]


class Refused(Exception):
    def __init__(self, code, field=None, reason=None):
        super().__init__(code)
        self.code, self.field, self.reason = code, field, reason


def invalid(field, reason):
    return Refused("report.layout_invalid", field, reason)


def length(field, text, signed=False):
    """Units of a length text, or Refused(report.layout_invalid, field, reason)."""
    negative = signed and text.startswith("-")
    if negative:
        text = text[1:]
    unit = text[-2:] if len(text) >= 2 else ""
    number = text[:-2]
    ok = unit in UNITS and number != ""
    if ok:
        int_part, dot, frac_part = number.partition(".")
        ok = int_part.isdigit() and int_part.isascii() and (not dot or (frac_part.isdigit() and frac_part.isascii()))
    if not ok:
        raise invalid(field, "not_a_length")
    value = Fraction(number) * UNITS[unit]
    if value.denominator != 1:
        raise invalid(field, "not_exact")
    if value > MAX_LENGTH:
        raise invalid(field, "too_large")
    return -int(value) if negative else int(value)


def pitch_units(units):
    if units < MIN_PITCH:
        raise invalid("grid_pitch", "below_minimum")
    if units > MAX_PITCH:
        raise invalid("grid_pitch", "too_large")
    return units


def pitch_text(text):
    return pitch_units(length("grid_pitch", text))


def snap(v, pitch):
    """v rounded to the nearest multiple of pitch, ties away from zero (exact, Fraction)."""
    q = Fraction(abs(v), pitch)
    whole = q.numerator // q.denominator
    if q - whole >= Fraction(1, 2):
        whole += 1
    return (whole * pitch) if v >= 0 else -(whole * pitch)


class World:
    def __init__(self, fixture, blocks):
        self.pages = [f"P{i}" for i in range(1, PAGES[fixture] + 1)]
        # block handle -> page handle, and page handle -> ordered block handles
        self.page_of = {}
        self.order = {p: [] for p in self.pages}
        for b in blocks:
            self.page_of[b["handle"]] = b["page"]
            self.order[b["page"]].append(b["handle"])
        self.frames = {}
        self.journal = 0

    def state(self):
        out = {}
        for b, p in sorted(self.page_of.items()):
            f = self.frames.get(b)
            out[b] = {"page": p, "frame": None if f is None else {k: str(v) for k, v in f.items()}}
        return out

    def move(self, params, pitch):
        given = {}
        for k in ABSOLUTE + DELTA:
            if k in params:
                given[k] = length(k, params[k], k in SIGNED)
        snapping = params.get("snap", False)
        if snapping:
            pitch_units(pitch)
        if params["report"] != "R":
            raise Refused("report.not_found")
        block = params["block"]
        if block not in self.page_of:
            raise Refused("report.not_found")
        page = params.get("page")
        if page is not None and page not in self.pages:
            raise Refused("report.not_found")
        base = self.frames.get(block)
        frame = {}
        for a, d in zip(ABSOLUTE, DELTA):
            if a in given:
                frame[a] = given[a]
            elif base is None:
                raise invalid(a, "no_frame")
            else:
                frame[a] = base[a] + given.get(d, 0)
        if snapping:
            left, top = snap(frame["x"], pitch), snap(frame["y"], pitch)
            right, bottom = snap(frame["x"] + frame["w"], pitch), snap(frame["y"] + frame["h"], pitch)
            frame = {"x": left, "y": top, "w": right - left, "h": bottom - top}
        for k in ("x", "y"):
            if abs(frame[k]) > MAX_LENGTH:
                raise invalid(k, "too_large")
        for k in ("w", "h"):
            if frame[k] < MIN_SIDE:
                raise invalid(k, "too_small")
            if frame[k] > MAX_LENGTH:
                raise invalid(k, "too_large")
        target = self.page_of[block] if page is None else page
        if target != self.page_of[block]:
            self.order[self.page_of[block]].remove(block)
            self.order[target].append(block)
            self.page_of[block] = target
        self.frames[block] = frame
        self.journal += 1
        out = {k: str(v) for k, v in frame.items()}
        out.update({"block": block, "page": target, "report": "R"})
        return out


def run(name, fixture, blocks, pitch, steps):
    w = World(fixture, blocks)
    out_steps = []
    for st in steps:
        params = st["params"]
        step_pitch = st.get("pitch", pitch)
        before = w.journal
        try:
            expect = {"result": w.move(params, step_pitch)}
        except Refused as e:
            expect = {"error": e.code}
            if e.field is not None:
                expect["field"], expect["reason"] = e.field, e.reason
        out_steps.append({
            "params": params,
            "pitch": str(step_pitch),
            "expect": expect,
            "journaled": w.journal == before + 1,
            "state": w.state(),
        })
    return {"name": name, "fixture": fixture, "blocks": blocks, "steps": out_steps}


def blk(handle, page, text):
    return {"handle": handle, "page": page, "text": text}


def mv(block="B1", report="R", **kw):
    p = {"report": report, "block": block}
    p.update(kw)
    return {"params": p}


def with_pitch(step, pitch):
    step["pitch"] = pitch
    return step


ONE_B1 = [blk("B1", "P1", "Placeholder block one")]
THREE = [
    blk("B1", "P1", "Placeholder block one"),
    blk("B2", "P2", "Placeholder block two"),
    blk("B3", "P1", "Placeholder block three"),
]

SCENARIOS = [
    # AC-1, AC-2, AC-3 in order on one_page_report, pitch 1 mm.
    ("ac1_ac2_ac3", "one_page_report", ONE_B1, 100, [
        mv(x="10.04mm", y="20mm", w="80mm", h="40mm", snap=True),
        mv(dx="0.35mm"),
        mv(w="0.5mm"),
    ]),
    # Snapping edges, not origin and size: ties away from zero on negative edges too.
    ("snap_edges", "one_page_report", ONE_B1, 250, [
        mv(x="-1.25mm", y="-3.75mm", w="10.1mm", h="5mm", snap=True),
        mv(x="1.24mm", y="1.25mm", w="2.51mm", h="2.49mm", snap=True),
        mv(dx="-0.01mm", snap=True),
        mv(x="0.6mm", w="0.3mm", h="1mm", y="0mm", snap=True),
        with_pitch(mv(x="0.6mm", y="0mm", w="1.3mm", h="1mm", snap=True), 10),
        with_pitch(mv(dx="0.04mm", dy="0.05mm", snap=True), 10),
        with_pitch(mv(dw="0.333mm"), 10),
        with_pitch(mv(dw="0.5in"), 10),
        with_pitch(mv(x="0.55in", snap=True), 2540),
    ]),
    # A frame may lie outside the body; bounds of the frame itself.
    ("outside_and_bounds", "one_page_report", ONE_B1, 100, [
        mv(x="-50mm", y="-20mm", w="300mm", h="500mm"),
        mv(x="2000mm"),
        mv(dx="0.01mm"),
        mv(dx="-4000mm"),
        mv(x="-2000mm", dw="1700mm"),
        mv(dw="0.01mm"),
        mv(w="1mm", h="1mm"),
        mv(dh="-0.01mm"),
        mv(h="2000.01mm"),
    ]),
    # A block without a frame needs every coordinate.
    ("no_frame", "one_page_report", ONE_B1, 100, [
        mv(dx="1mm"),
        mv(x="1mm", w="10mm", h="10mm"),
        mv(x="1mm", y="1mm", w="10mm"),
        mv(x="1mm", y="1mm", w="10mm", h="10mm"),
        mv(dy="1mm"),
    ]),
    # Length syntax and exactness, in the parse order of the rules.
    ("lengths", "one_page_report", ONE_B1, 100, [
        mv(x="10.005mm", y="1mm", w="10mm", h="10mm"),
        mv(x="1mm", y="+1mm", w="10mm", h="10mm"),
        mv(x="1mm", y="1mm", w="-10mm", h="10mm"),
        mv(x="1mm", y="1mm", w="10mm", h="10 mm"),
        mv(x="1mm", y="1mm", w="10mm", h="0.01in"),
        mv(x="--1mm", y="1mm", w="10mm", h="10mm"),
        mv(x="-0mm", y="0.05in", w="1in", h="25.4mm"),
        mv(dx="1cm"),
        mv(dy="-2000.01mm"),
        mv(dw="0.001mm"),
        mv(dh="-1.5in"),
    ]),
    # Not-found paths and their order after the length checks.
    ("not_found", "three_page_report", THREE, 100, [
        mv(report="R?", x="1mm", y="1mm", w="10mm", h="10mm"),
        mv(block="B?", x="1mm", y="1mm", w="10mm", h="10mm"),
        mv(x="1mm", y="1mm", w="10mm", h="10mm", page="X"),
        mv(block="B?", x="1.001mm", y="1mm", w="10mm", h="10mm"),
        mv(block="B?", dx="1mm"),
    ]),
    # Moving to another page: membership changes, the frame stays body-relative.
    ("move_page", "three_page_report", THREE, 100, [
        mv(x="5mm", y="5mm", w="50mm", h="20mm", page="P3"),
        mv(dy="10mm", page="P3"),
        mv(block="B2", x="0mm", y="0mm", w="10mm", h="10mm", page="P2"),
        mv(block="B3", x="0mm", y="0mm", w="10mm", h="10mm", page="P2", snap=True),
        mv(block="B2", dx="1mm", page="P1"),
        mv(dx="1mm", page="X"),
    ]),
]

PITCH_TEXTS = ["1mm", "0.1mm", "0.10mm", "0.05mm", "0.09mm", "0mm", "0.05in", "2000mm", "2000.01mm",
               "0.15mm", "1in", "0.001mm", "-1mm", "1", "1 mm"]
PITCH_UNITS = [0, 5, 9, 10, 11, 100, 2540, 200000, 200001, -100]
SNAP_CASES = [(v, p) for p in (10, 100, 250, 2540, 200000) for v in
              (0, 1, 4, 5, 6, 49, 50, 51, 99, 100, 124, 125, 126, 1269, 1270, 1271, 99999, 100000, 100001, 200000)]
SNAP_CASES += [(-v, p) for (v, p) in SNAP_CASES if v != 0]


def pitch_text_case(text):
    try:
        return {"text": text, "units": str(pitch_text(text))}
    except Refused as e:
        return {"text": text, "field": e.field, "error": e.reason}


def pitch_units_case(units):
    try:
        return {"units": str(units), "pitch": str(pitch_units(units))}
    except Refused as e:
        return {"units": str(units), "error": e.reason}


def build():
    cases = {
        "default_pitch": str(DEFAULT_PITCH),
        "pitch_texts": [pitch_text_case(t) for t in PITCH_TEXTS],
        "pitch_units": [pitch_units_case(u) for u in PITCH_UNITS],
        "snaps": [{"value": str(v), "pitch": str(p), "snapped": str(snap(v, p))} for v, p in SNAP_CASES],
        "scenarios": [run(*s) for s in SCENARIOS],
    }
    dump = lambda v: json.dumps(v, indent=1, sort_keys=True, ensure_ascii=True) + "\n"
    return {OUT_CASES: dump(cases)}


def self_check(files):
    """The acceptance criteria as the spec states them, against the oracle's output."""
    doc = json.loads(files[OUT_CASES])
    sc = {s["name"]: s["steps"] for s in doc["scenarios"]}
    ac = sc["ac1_ac2_ac3"]
    assert ac[0]["expect"]["result"] == {
        "block": "B1", "page": "P1", "report": "R", "x": "1000", "y": "2000", "w": "8000", "h": "4000"}
    assert ac[0]["journaled"]
    assert ac[1]["expect"]["result"]["x"] == "1035" and ac[1]["journaled"]
    assert ac[2]["expect"] == {"error": "report.layout_invalid", "field": "w", "reason": "too_small"}
    assert not ac[2]["journaled"]
    assert ac[2]["state"]["B1"]["frame"] == {"x": "1035", "y": "2000", "w": "8000", "h": "4000"}
    p = {c["text"]: c for c in doc["pitch_texts"]}
    assert p["0.05mm"] == {"text": "0.05mm", "field": "grid_pitch", "error": "below_minimum"}
    assert p["1mm"]["units"] == "100" and p["0.1mm"]["units"] == "10"
    # Ties away from zero.
    s = {(c["value"], c["pitch"]): c["snapped"] for c in doc["snaps"]}
    assert s[("50", "100")] == "100" and s[("-50", "100")] == "-100" and s[("49", "100")] == "0"
    assert s[("125", "250")] == "250" and s[("-125", "250")] == "-250"


def main():
    files = build()
    self_check(files)
    if "--check" in sys.argv[1:]:
        stale = [p for p, text in files.items() if not p.exists() or p.read_text(encoding="utf-8") != text]
        for p in stale:
            print(f"{p} is stale; run {Path(__file__).name}", file=sys.stderr)
        if stale:
            return 1
        print("block frame fixtures up to date")
        return 0
    for p, text in files.items():
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8", newline="\n")
        print(f"wrote {p}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
