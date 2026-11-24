#!/usr/bin/env python3
"""Independent oracle for RPT.BALLOON.REG (balloon register and renumbering).

Written from the rules of spec/rpt/balloon/reg/NODE.md with the Python standard library
only; nothing here is produced by running the Rust code. Writes two_sheet_drawing.json
(CC-BY-4.0, invented data, D-038):

  sheets           the synthetic drawing revision of the fixture: two blank A3 landscape
                   sheets S1 and S2, 42000 x 29700 in 0.01 mm (420 x 297 mm), origin top-left.
  characteristics  ten stub characteristics C1..C10.
  scenarios        ordered command calls with the expected outcome of each call: either the
                   error code, or the result (assigned number, renumber changes), followed by
                   the full expected balloon list (ascending number) and journal length.

Balloons are named by handles B1, B2, ... in the order the oracle creates them; the Rust test
maps each handle to the UUIDv7 the handler generated. All integers are written as decimal
strings (the wire form of fid-report, which holds no float).

Rules (the order of checks is the order of the requirements, REQ-002 before REQ-003):

  add       zone longer than 16 bytes or holding NUL -> report.text_invalid;
            number 0, or mode insert without a number -> report.bad_params;
            sheet unknown or not on the drawing revision -> report.not_found;
            anchor or label outside 0..=width x 0..=height -> report.balloon_off_sheet;
            characteristic unknown -> report.not_found;
            mode set with a number in use -> report.balloon_number_taken;
            characteristic already ballooned -> report.char_already_ballooned;
            else number = given, or highest in use + 1 (1 if none); mode insert first adds
            one to every balloon numbered at or above the target.
  move      no field given -> report.bad_params; balloon unknown or removed ->
            report.not_found; new sheet not on the balloon's drawing revision ->
            report.not_found; merged anchor or label off the (new) sheet ->
            report.balloon_off_sheet.
  renumber  set/insert without balloon or number, number 0, or compact with either ->
            report.bad_params; balloon unknown or not on the revision -> report.not_found;
            set: same number -> no change; number in use -> report.balloon_number_taken;
            insert: target takes the number, every other balloon at or above it + 1;
            compact: 1..n in current order. Changes list (balloon, old, new) only for
            balloons whose number changed, ascending by new number.
  relink    balloon unknown -> report.not_found; characteristic unknown -> report.not_found;
            characteristic held by another balloon -> report.char_already_ballooned;
            same characteristic -> no change (still one event).
  remove    balloon unknown or removed -> report.not_found; else the number is free and the
            ID is retired for ever.

Every successful W2 command journals exactly one event; every failure journals nothing.
"""

import json
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "two_sheet_drawing.json"

SHEETS = {"S1": (42000, 29700), "S2": (42000, 29700)}
CHARS = [f"C{i}" for i in range(1, 11)]
MAX_ZONE = 16
U32_MAX = 2**32 - 1


class Fail(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


class Register:
    def __init__(self):
        self.balloons = {}  # handle -> dict(number, char, sheet, anchor, label, zone)
        self.retired = set()
        self.journal = 0
        self.next_handle = 1
        self.seen_rev = False

    # -- checks -----------------------------------------------------------------------
    def check_point(self, sheet, pt):
        w, h = SHEETS[sheet]
        x, y = pt
        if x < 0 or y < 0 or x > w or y > h:
            raise Fail("report.balloon_off_sheet")

    def number_in_use(self, n):
        return any(b["number"] == n for b in self.balloons.values())

    def holder_of(self, char):
        for h, b in self.balloons.items():
            if b["char"] == char:
                return h
        return None

    # -- commands ---------------------------------------------------------------------
    def add(self, p):
        zone = p.get("zone")
        if zone is not None and (len(zone.encode("utf-8")) > MAX_ZONE or "\0" in zone):
            raise Fail("report.text_invalid")
        number = p.get("number")
        mode = p.get("mode", "set")
        if number == 0 or (mode == "insert" and number is None):
            raise Fail("report.bad_params")
        if p["sheet"] not in SHEETS:
            raise Fail("report.not_found")
        self.check_point(p["sheet"], p["anchor"])
        self.check_point(p["sheet"], p["label"])
        if p["char"] not in CHARS:
            raise Fail("report.not_found")
        if mode == "set" and number is not None and self.number_in_use(number):
            raise Fail("report.balloon_number_taken")
        if self.holder_of(p["char"]) is not None:
            raise Fail("report.char_already_ballooned")
        if number is None:
            number = max((b["number"] for b in self.balloons.values()), default=0) + 1
        if mode == "insert":
            for b in self.balloons.values():
                if b["number"] >= number:
                    b["number"] += 1
        handle = f"B{self.next_handle}"
        self.next_handle += 1
        self.balloons[handle] = {
            "number": number,
            "char": p["char"],
            "sheet": p["sheet"],
            "anchor": p["anchor"],
            "label": p["label"],
            "zone": zone,
        }
        self.seen_rev = True
        self.journal += 1
        return {"handle": handle, "number": number}

    def move(self, p):
        if p.get("sheet") is None and p.get("anchor") is None and p.get("label") is None:
            raise Fail("report.bad_params")
        b = self.balloons.get(p["balloon"])
        if b is None:
            raise Fail("report.not_found")
        sheet = p.get("sheet") or b["sheet"]
        if sheet not in SHEETS:
            raise Fail("report.not_found")
        anchor = p.get("anchor") or b["anchor"]
        label = p.get("label") or b["label"]
        self.check_point(sheet, anchor)
        self.check_point(sheet, label)
        b["sheet"], b["anchor"], b["label"] = sheet, anchor, label
        self.journal += 1
        return {"number": b["number"]}

    def renumber(self, p):
        mode = p["mode"]
        balloon, number = p.get("balloon"), p.get("number")
        if mode in ("set", "insert"):
            if balloon is None or number is None or number == 0:
                raise Fail("report.bad_params")
        elif balloon is not None or number is not None:
            raise Fail("report.bad_params")
        if not self.seen_rev:
            raise Fail("report.not_found")
        before = {h: b["number"] for h, b in self.balloons.items()}
        if mode in ("set", "insert") and balloon not in self.balloons:
            raise Fail("report.not_found")
        if mode == "set":
            if before[balloon] != number:
                if self.number_in_use(number):
                    raise Fail("report.balloon_number_taken")
                self.balloons[balloon]["number"] = number
        elif mode == "insert":
            for h, b in self.balloons.items():
                if h != balloon and b["number"] >= number:
                    if b["number"] == U32_MAX:
                        raise Fail("report.bad_params")
            for h, b in self.balloons.items():
                if h != balloon and b["number"] >= number:
                    b["number"] += 1
            self.balloons[balloon]["number"] = number
        else:
            order = sorted(self.balloons, key=lambda h: self.balloons[h]["number"])
            for i, h in enumerate(order, start=1):
                self.balloons[h]["number"] = i
        changes = [
            [h, before[h], b["number"]] for h, b in self.balloons.items() if before[h] != b["number"]
        ]
        changes.sort(key=lambda c: c[2])
        self.journal += 1
        return {"changes": changes}

    def relink(self, p):
        b = self.balloons.get(p["balloon"])
        if b is None:
            raise Fail("report.not_found")
        if p["char"] not in CHARS:
            raise Fail("report.not_found")
        holder = self.holder_of(p["char"])
        if holder is not None and holder != p["balloon"]:
            raise Fail("report.char_already_ballooned")
        b["char"] = p["char"]
        self.journal += 1
        return {"number": b["number"]}

    def remove(self, p):
        if p["balloon"] not in self.balloons:
            raise Fail("report.not_found")
        del self.balloons[p["balloon"]]
        self.retired.add(p["balloon"])
        self.journal += 1
        return {}

    # -- views ------------------------------------------------------------------------
    def listing(self):
        rows = []
        for h, b in sorted(self.balloons.items(), key=lambda kv: kv[1]["number"]):
            rows.append(
                {
                    "balloon": h,
                    "number": str(b["number"]),
                    "characteristic": b["char"],
                    "sheet": b["sheet"],
                    "anchor": [str(b["anchor"][0]), str(b["anchor"][1])],
                    "label": [str(b["label"][0]), str(b["label"][1])],
                    "zone": b["zone"],
                }
            )
        return rows


def s(v):
    """Integers and points to the wire form (decimal strings); other values unchanged."""
    if isinstance(v, bool) or v is None:
        return v
    if isinstance(v, int):
        return str(v)
    if isinstance(v, (list, tuple)):
        return [s(x) for x in v]
    if isinstance(v, dict):
        return {k: s(x) for k, x in v.items()}
    return v


def run(reg, calls):
    steps = []
    for ac, op, params in calls:
        try:
            res = getattr(reg, op)(params)
            expect = {"ok": s(res)}
        except Fail as f:
            expect = {"error": f.code}
        steps.append(
            {
                "ac": ac,
                "op": op,
                "params": s(params),
                "expect": expect,
                "list": reg.listing(),
                "journal_len": str(reg.journal),
            }
        )
    return steps


def acceptance():
    """AC-1 to AC-5 in order (AC-7 replays the journal this leaves)."""
    calls = []
    for i in range(1, 11):
        sheet = "S1" if i <= 5 else "S2"
        k = (i - 1) % 5
        anchor = (4000 * k + 3000, 5000 + 1000 * k)
        label = (anchor[0] + 1500, 2500)
        calls.append(("AC-1", "add", {"char": f"C{i}", "sheet": sheet, "anchor": anchor, "label": label}))
    calls.append(("AC-2", "add", {"char": "C3", "sheet": "S1", "anchor": (1000, 1000), "label": (2000, 1000)}))
    calls.append(
        ("AC-2", "add", {"char": "C5", "sheet": "S1", "anchor": (1000, 1000), "label": (2000, 1000), "number": 4})
    )
    calls.append(("AC-3", "remove", {"balloon": "B7"}))
    calls.append(("AC-3", "renumber", {"mode": "set", "balloon": "B9", "number": 7}))
    calls.append(
        (
            "AC-4",
            "add",
            {"char": "C7", "sheet": "S2", "anchor": (20000, 15000), "label": (21500, 14000), "number": 2, "mode": "insert"},
        )
    )
    calls.append(("AC-5", "renumber", {"mode": "compact"}))
    return calls


def off_sheet():
    """AC-6 and the bounds edges of REQ-008 on a fresh register (sheet S1 is 42000 x 29700)."""
    base = {"char": "C1", "sheet": "S1", "anchor": (100, 100), "label": (200, 200)}
    return [
        ("AC-6", "add", dict(base, anchor=(42001, 100))),
        ("REQ-008", "add", dict(base, label=(100, 29701))),
        ("REQ-008", "add", dict(base, anchor=(-1, 100))),
        ("REQ-008", "add", dict(base, label=(100, -1))),
        ("REQ-008", "add", dict(base, anchor=(42000, 29700), label=(0, 0))),
        ("REQ-008", "move", {"balloon": "B1", "anchor": (42001, 0)}),
        ("REQ-008", "move", {"balloon": "B1", "sheet": "S2", "label": (41999, 29699)}),
    ]


def extras():
    """Relink, remove/ID retirement, set/insert edge cases, zone and parameter errors."""
    pt = {"anchor": (1000, 1000), "label": (2000, 2000)}
    return [
        ("REQ-001", "renumber", {"mode": "compact"}),
        ("REQ-001", "add", dict(pt, char="C1", sheet="S1")),
        ("REQ-002", "add", dict(pt, char="C2", sheet="S1", number=5)),
        ("REQ-001", "add", dict(pt, char="C3", sheet="S2")),
        ("REQ-002", "add", dict(pt, char="C4", sheet="S1", number=0)),
        ("REQ-002", "add", dict(pt, char="C4", sheet="S1", mode="insert")),
        ("REQ-003", "add", dict(pt, char="C99", sheet="S1")),
        ("REQ-010", "add", dict(pt, char="C4", sheet="S1", zone="A1234567890123456")),
        ("REQ-010", "add", dict(pt, char="C4", sheet="S1", zone="SHEET2-ZONE-B4/x")),
        ("REQ-003", "relink", {"balloon": "B1", "char": "C2"}),
        ("REQ-003", "relink", {"balloon": "B1", "char": "C99"}),
        ("REQ-003", "relink", {"balloon": "B1", "char": "C9"}),
        ("REQ-003", "relink", {"balloon": "B1", "char": "C9"}),
        ("REQ-003", "add", dict(pt, char="C1", sheet="S2")),
        ("REQ-004", "renumber", {"mode": "set", "balloon": "B2", "number": 6}),
        ("REQ-004", "renumber", {"mode": "set", "balloon": "B2", "number": 5}),
        ("REQ-004", "renumber", {"mode": "set", "balloon": "B2", "number": 0}),
        ("REQ-004", "renumber", {"mode": "set", "balloon": "B2"}),
        ("REQ-006", "renumber", {"mode": "compact", "number": 1}),
        ("REQ-005", "renumber", {"mode": "insert", "balloon": "B5", "number": 1}),
        ("REQ-005", "renumber", {"mode": "insert", "balloon": "B1", "number": 3}),
        ("REQ-009", "remove", {"balloon": "B2"}),
        ("REQ-009", "remove", {"balloon": "B2"}),
        ("REQ-009", "move", {"balloon": "B2", "anchor": (10, 10)}),
        ("REQ-009", "relink", {"balloon": "B2", "char": "C5"}),
        ("REQ-009", "renumber", {"mode": "set", "balloon": "B2", "number": 9}),
        ("REQ-009", "add", dict(pt, char="C2", sheet="S1")),
        ("REQ-009", "add", dict(pt, char="C6", sheet="S1", number=7)),
        ("REQ-008", "move", {"balloon": "B6"}),
        ("REQ-006", "renumber", {"mode": "compact"}),
    ]


def build():
    doc = {
        "about": "Independent oracle for RPT.BALLOON.REG, generated by make_fixture.py from the spec "
        "rules with the Python standard library; never produced by the Rust code. Invented data "
        "(D-038), CC-BY-4.0.",
        "sheets": [
            {"key": k, "width_cmm": str(w), "height_cmm": str(h)} for k, (w, h) in sorted(SHEETS.items())
        ],
        "characteristics": CHARS,
        "scenarios": [
            {"name": "acceptance", "steps": run(Register(), acceptance())},
            {"name": "off_sheet", "steps": run(Register(), off_sheet())},
            {"name": "extras", "steps": run(Register(), extras())},
        ],
    }
    return json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n"


def self_check(doc):
    """The acceptance criteria as stated in the spec, checked against the oracle's output."""
    steps = json.loads(doc)["scenarios"][0]["steps"]

    def nums(step):
        return [(r["balloon"], int(r["number"]), r["characteristic"]) for r in step["list"]]

    ac1 = steps[9]
    assert [n for _, n, _ in nums(ac1)] == list(range(1, 11)) and ac1["journal_len"] == "10"
    assert steps[10]["expect"] == {"error": "report.char_already_ballooned"}
    assert steps[11]["expect"] == {"error": "report.balloon_number_taken"}
    assert steps[11]["journal_len"] == "10"
    ac3 = nums(steps[13])
    assert [n for _, n, _ in ac3] == [1, 2, 3, 4, 5, 6, 7, 8, 10] and ac3[6][0] == "B9"
    ac4 = nums(steps[14])
    assert ac4[1] == ("B11", 2, "C7") and [n for _, n, _ in ac4] == [1, 2, 3, 4, 5, 6, 7, 8, 9, 11]
    ac5 = nums(steps[15])
    assert [n for _, n, _ in ac5] == list(range(1, 11)) and [h for h, _, _ in ac5] == [h for h, _, _ in ac4]
    off = json.loads(doc)["scenarios"][1]["steps"]
    assert off[0]["expect"] == {"error": "report.balloon_off_sheet"} and off[0]["journal_len"] == "0"


def main() -> int:
    text = build()
    self_check(text)
    if "--check" in sys.argv[1:]:
        have = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if have != text:
            print(f"{OUT} is stale; run {Path(__file__).name}", file=sys.stderr)
            return 1
        print(f"{OUT.name} up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
