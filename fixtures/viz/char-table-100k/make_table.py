#!/usr/bin/env python3
"""Generator and reference model for fixtures/viz/char-table-100k (spec node VIZ.SHELL.SPKTABLE).

The table itself (10^5 rows x 14 columns, about 8 MB of text) is not committed: it is
generated from SplitMix64 seeded with 9, by this script and, independently, by the Rust crate
(``crates/fid-spike-ui/src/table``). This script writes next to itself:

* ``digest.txt``: row count and the FNV-1a 64 digest of the whole table as TSV (header line,
  then one line per row, each ending in a newline), so the Rust generator can prove it
  produces the same bytes;
* ``sample.tsv``: the first 50 rows, every 997th row and the last 50 rows, verbatim;
* ``sort.tsv``: the row order after sorting by a column (count, digest of the ids and the
  first and last 20 ids);
* ``filter.tsv``: the rows shown for filter texts (alone and after a sort);
* ``keyboard.tsv``: the scripted keyboard sequence of AC-4 with the state expected after
  each step, from the grid rules below (this model is written independently of the crate);
* ``copy.tsv``: the clipboard text of the first Copy step in the keyboard sequence.

Run ``python make_table.py --check`` to verify that the committed files match.

Stdlib only; ``random`` is not used so the output does not depend on the Python version.
Generated numbers are integers in units of 0.0001 and printed with four decimals; edited
cells are compared and subtracted with ``decimal.Decimal`` (an independent route from the
crate's scaled integers).

Generation (per row id 1..100000, draws in this order):

1. ``c = below(8)`` picks the characteristic from CHARS (name, unit, class).
2. ``f = below(6)``, ``k = 1 + below(400)``: feature ``"<FEATURES[f]> <k>"``.
3. ``d = below(2000000)``: nominal ``10000 + d`` (size), ``d % 900000`` (angle) or 0 (form).
4. ``t = TOLS[below(4)]``: upper ``t``; lower ``-t`` (size, angle) or 0 (form).
5. ``measured = below(50) != 0``.
6. ``span = t * 5 // 4``; ``e = below(2 * span + 1)``: deviation ``e - span`` (size, angle) or
   ``e // 2`` (form). Actual is nominal plus deviation. Unmeasured rows show ``-`` for actual
   and deviation.
7. ``uec = 5 + below(46)``.
8. ``method = METHODS[below(4)]``.
9. ``n = below(10)``; if ``n == 0`` the note is ``NOTES[below(5)]``, else empty.

Balloon equals the id. Status: unmeasured -> "Not measured"; deviation above upper or below
lower -> "Fail"; ``|2 dev - upper - lower| * 4 > 3 (upper - lower)`` -> "Warn" (the outer
eighth of the band on either side); otherwise "Pass".

Grid rules (the crate documents the same rules in ``table/grid.rs``):

* Columns in display order start as COLUMNS; the first two display positions are frozen and
  a column never moves across that boundary (MoveLeft / MoveRight swap the focused column with
  its neighbour on the same side; otherwise nothing). Widths start at WIDTHS; Narrow / Widen
  change the focused column by 10 px within [40, 600].
* The view is the row ids in sort order (none: ascending id; ascending: key then id;
  descending: key descending, then ascending id), keeping rows where the filter (ASCII
  case-insensitive) is a substring of any of the row's 14 cell texts. Numeric columns compare
  as exact decimals, ``-`` after every number; status by Pass < Warn < Fail < Not measured;
  text columns by code point.
* Focus is (row id, column); it starts on the first row and the first column. After a sort or
  filter change (a "refresh") the anchor is cleared and the focus keeps its row if the row is
  still shown, else moves to the first shown row (none if the view is empty).
* Grid mode: Up / Down / PageUp / PageDown (20 rows) / CtrlHome / CtrlEnd move the row, Left /
  Right / Home / End the column, clamped. Shift+arrows set the anchor to the focus if unset
  and move. Every grid action except Shift moves, Copy and FlyTo clears the anchor. SortAsc /
  SortDesc sort by the focused column, SortClear removes the sort. FocusFilter enters filter
  mode. Enter or F2 on Nominal, Upper or Lower starts editing with the stored text; Type on
  such a cell starts editing with an empty buffer and types into it; on other cells both do
  nothing. Escape clears the anchor. Copy puts the rectangle between anchor (or focus) and
  focus on the clipboard as TSV: a header line of column titles, then one line per row, each
  line ending in a newline; control characters become spaces. FlyTo records the focused row.
* Filter mode: Type appends (at most 64 characters), Backspace removes the last character,
  both refresh; Enter returns to the grid; Escape clears the filter, refreshes and returns.
* Edit mode: Type appends (the buffer holds at most 26 characters), Backspace removes the last
  character; Escape cancels. Enter commits when the buffer matches
  ``-?[0-9]{1,12}(.[0-9]{1,12})?`` (else error "not-decimal") and upper is not below lower
  (else "upper-below-lower"); the typed string is stored exactly, deviation becomes actual
  minus nominal with the larger of their decimal places, and status is recomputed. Editing
  never re-sorts or re-filters the view.
"""

import os
import re
import sys
from decimal import Decimal

HERE = os.path.dirname(os.path.abspath(__file__))
SEED = 9
ROWS = 100000
MASK = (1 << 64) - 1

CHARS = [
    ("Diameter", "mm", "size"), ("Distance", "mm", "size"), ("Angle", "deg", "angle"),
    ("Position", "mm", "form"), ("Flatness", "mm", "form"), ("Profile", "mm", "form"),
    ("Roundness", "mm", "form"), ("Perpendicularity", "mm", "form"),
]
FEATURES = ["Hole", "Plane", "Slot", "Cylinder", "Boss", "Edge"]
METHODS = ["CMM tactile", "CMM scan", "Optical", "Gauge"]
NOTES = ["datum A", "re-measured", "operator check", "first article", "see deviation report"]
TOLS = [50, 100, 200, 500]

COLUMNS = ["id", "balloon", "characteristic", "nominal", "upper", "lower", "actual",
           "deviation", "status", "uec", "unit", "method", "feature", "note"]
TITLES = {
    "id": "ID", "balloon": "Balloon", "characteristic": "Characteristic", "nominal": "Nominal",
    "upper": "Upper tol.", "lower": "Lower tol.", "actual": "Actual", "deviation": "Deviation",
    "status": "Status", "uec": "UEC", "unit": "Unit", "method": "Method", "feature": "Feature",
    "note": "Note",
}
WIDTHS = {
    "id": 70, "balloon": 70, "characteristic": 150, "nominal": 90, "upper": 90, "lower": 90,
    "actual": 90, "deviation": 90, "status": 120, "uec": 80, "unit": 50, "method": 110,
    "feature": 100, "note": 160,
}
NUMERIC = {"nominal", "upper", "lower", "actual", "deviation", "uec"}
EDITABLE = {"nominal", "upper", "lower"}
STATUS_RANK = {"Pass": 0, "Warn": 1, "Fail": 2, "Not measured": 3}
FROZEN = 2
PAGE_ROWS = 20
MAX_FILTER = 64
MAX_EDIT = 26
DECIMAL = re.compile(r"-?[0-9]{1,12}(\.[0-9]{1,12})?")


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n


def fnv1a(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & MASK
    return h


def fmt4(u):
    a = abs(u)
    return "%s%d.%04d" % ("-" if u < 0 else "", a // 10000, a % 10000)


def status_of(dev, upper, lower):
    if dev is None:
        return "Not measured"
    if dev > upper or dev < lower:
        return "Fail"
    if abs(2 * dev - upper - lower) * 4 > 3 * (upper - lower):
        return "Warn"
    return "Pass"


# ---------------------------------------------------------------------------------------
# Table generation


def generate():
    rng = SplitMix64(SEED)
    rows = []
    for rid in range(1, ROWS + 1):
        name, unit, cls = CHARS[rng.below(8)]
        feat = FEATURES[rng.below(6)]
        feature = "%s %d" % (feat, 1 + rng.below(400))
        d = rng.below(2000000)
        nominal = 10000 + d if cls == "size" else (d % 900000 if cls == "angle" else 0)
        t = TOLS[rng.below(4)]
        upper, lower = t, (0 if cls == "form" else -t)
        measured = rng.below(50) != 0
        span = t * 5 // 4
        e = rng.below(2 * span + 1)
        dev = (e // 2) if cls == "form" else e - span
        uec = 5 + rng.below(46)
        method = METHODS[rng.below(4)]
        note = NOTES[rng.below(5)] if rng.below(10) == 0 else ""
        rows.append({
            "id": rid, "balloon": rid, "characteristic": name,
            "nominal": fmt4(nominal), "upper": fmt4(upper), "lower": fmt4(lower),
            "actual": fmt4(nominal + dev) if measured else "-",
            "deviation": fmt4(dev) if measured else "-",
            "status": status_of(dev if measured else None, upper, lower),
            "uec": fmt4(uec), "unit": unit, "method": method, "feature": feature, "note": note,
        })
    return rows


def text(row, col):
    v = row[col]
    return str(v) if isinstance(v, int) else v


def tsv_line(row):
    return "\t".join(text(row, c) for c in COLUMNS) + "\n"


HEADER = "\t".join(COLUMNS) + "\n"


# ---------------------------------------------------------------------------------------
# Sorting and filtering


def sort_key(row, col):
    if col in ("id", "balloon"):
        return row[col]
    if col in NUMERIC:
        v = row[col]
        return (1, Decimal(0)) if v == "-" else (0, Decimal(v))
    if col == "status":
        return STATUS_RANK[row[col]]
    return row[col]


ASCII_LOWER = {c: c + 32 for c in range(ord("A"), ord("Z") + 1)}


def ascii_lower(s):
    return s.translate(ASCII_LOWER)


def view_of(rows, sort, filt):
    ids = [r["id"] for r in rows]  # rows are in id order
    if sort is not None:
        col, direction = sort
        ids = sorted(ids, key=lambda i: sort_key(rows[i - 1], col), reverse=(direction == "desc"))
    if filt:
        needle = ascii_lower(filt)
        ids = [i for i in ids if any(needle in ascii_lower(text(rows[i - 1], c)) for c in COLUMNS)]
    return ids


def ids_digest(ids):
    return fnv1a("".join("%d\n" % i for i in ids).encode("ascii"))


# ---------------------------------------------------------------------------------------
# The grid model (keyboard sequence)


class Grid:
    def __init__(self, rows):
        self.rows = rows
        self.order = list(COLUMNS)
        self.widths = dict(WIDTHS)
        self.sort = None
        self.filter = ""
        self.view = view_of(rows, None, "")
        self.focus_row = self.view[0] if self.view else None
        self.focus_col = self.order[0]
        self.anchor = None
        self.mode = "grid"
        self.buf = ""
        self.error = None
        self.clipboard = None
        self.fly_to = None

    def row(self, rid):
        return self.rows[rid - 1]

    def refresh(self):
        self.view = view_of(self.rows, self.sort, self.filter)
        self.anchor = None
        if self.focus_row not in set(self.view):
            self.focus_row = self.view[0] if self.view else None

    def pos(self):
        return self.view.index(self.focus_row)

    def move_row(self, to):
        if self.focus_row is None:
            return
        to = max(0, min(len(self.view) - 1, to))
        self.focus_row = self.view[to]

    def move_col(self, to):
        to = max(0, min(len(self.order) - 1, to))
        self.focus_col = self.order[to]

    def editable(self):
        return self.focus_row is not None and self.focus_col in EDITABLE

    def type_into(self, s, limit, current):
        for ch in s:
            if len(current) < limit:
                current += ch
        return current

    def copy(self):
        if self.focus_row is None:
            return
        a_row, a_col = self.anchor if self.anchor else (self.focus_row, self.focus_col)
        r0, r1 = sorted((self.view.index(a_row), self.pos()))
        c0, c1 = sorted((self.order.index(a_col), self.order.index(self.focus_col)))
        cols = self.order[c0:c1 + 1]

        def clean(s):
            return "".join(" " if (ord(ch) < 32 or 127 <= ord(ch) < 160) else ch for ch in s)

        lines = ["\t".join(TITLES[c] for c in cols)]
        for rid in self.view[r0:r1 + 1]:
            lines.append("\t".join(clean(text(self.row(rid), c)) for c in cols))
        self.clipboard = "".join(line + "\n" for line in lines)

    def commit(self):
        col = self.focus_col
        if not DECIMAL.fullmatch(self.buf):
            self.error = "not-decimal"
            return
        r = dict(self.row(self.focus_row))
        r[col] = self.buf
        if Decimal(r["upper"]) < Decimal(r["lower"]):
            self.error = "upper-below-lower"
            return
        if r["actual"] == "-":
            r["deviation"] = "-"
            r["status"] = "Not measured"
        else:
            dev = Decimal(r["actual"]) - Decimal(r["nominal"])
            s = format(dev, "f")
            r["deviation"] = s[1:] if s.startswith("-") and dev == 0 else s
            r["status"] = status_of(dev, Decimal(r["upper"]), Decimal(r["lower"]))
        self.rows[self.focus_row - 1] = r
        self.mode, self.buf, self.error = "grid", "", None

    def apply(self, key, arg):
        if self.mode == "filter":
            if key == "Type":
                self.filter = self.type_into(arg, MAX_FILTER, self.filter)
                self.refresh()
            elif key == "Backspace":
                self.filter = self.filter[:-1]
                self.refresh()
            elif key == "Enter":
                self.mode = "grid"
            elif key == "Escape":
                self.filter = ""
                self.refresh()
                self.mode = "grid"
            return
        if self.mode == "edit":
            if key == "Type":
                self.buf = self.type_into(arg, MAX_EDIT, self.buf)
                self.error = None
            elif key == "Backspace":
                self.buf = self.buf[:-1]
                self.error = None
            elif key == "Enter":
                self.commit()
            elif key == "Escape":
                self.mode, self.buf, self.error = "grid", "", None
            return
        if not (key.startswith("Shift") or key in ("Copy", "FlyTo")):
            self.anchor = None
        if key.startswith("Shift"):
            if self.focus_row is None:
                return
            if self.anchor is None:
                self.anchor = (self.focus_row, self.focus_col)
            key = key[len("Shift"):]
        cpos = self.order.index(self.focus_col)
        if key in ("Up", "Down", "PageUp", "PageDown", "CtrlHome", "CtrlEnd"):
            if self.focus_row is None:
                return
            p = self.pos()
            to = {"Up": p - 1, "Down": p + 1, "PageUp": p - PAGE_ROWS, "PageDown": p + PAGE_ROWS,
                  "CtrlHome": 0, "CtrlEnd": len(self.view) - 1}[key]
            self.move_row(to)
        elif key in ("Left", "Right", "Home", "End"):
            self.move_col({"Left": cpos - 1, "Right": cpos + 1, "Home": 0, "End": len(self.order) - 1}[key])
        elif key in ("SortAsc", "SortDesc"):
            self.sort = (self.focus_col, "asc" if key == "SortAsc" else "desc")
            self.refresh()
        elif key == "SortClear":
            self.sort = None
            self.refresh()
        elif key == "FocusFilter":
            self.mode = "filter"
        elif key == "Type":
            if self.editable():
                self.mode, self.buf, self.error = "edit", "", None
                self.buf = self.type_into(arg, MAX_EDIT, self.buf)
        elif key in ("Enter", "F2"):
            if self.editable():
                self.mode, self.buf, self.error = "edit", self.row(self.focus_row)[self.focus_col], None
        elif key == "Copy":
            self.copy()
        elif key == "FlyTo":
            if self.focus_row is not None:
                self.fly_to = self.focus_row
        elif key in ("MoveLeft", "MoveRight"):
            to = cpos - 1 if key == "MoveLeft" else cpos + 1
            if 0 <= to < len(self.order) and (to < FROZEN) == (cpos < FROZEN):
                self.order[cpos], self.order[to] = self.order[to], self.order[cpos]
        elif key in ("Narrow", "Widen"):
            w = self.widths[self.focus_col] + (10 if key == "Widen" else -10)
            self.widths[self.focus_col] = max(40, min(600, w))
        elif key == "Escape":
            pass  # the anchor is already cleared
        else:
            raise ValueError(key)

    # Accessibility facts of the focused cell (the crate's CellA11y).
    def header_label(self):
        c = self.focus_col
        if self.sort is not None and self.sort[0] == c:
            return "%s, sorted %s" % (TITLES[c], "ascending" if self.sort[1] == "asc" else "descending")
        return TITLES[c]

    def label(self):
        r = self.row(self.focus_row)
        c = self.focus_col
        s = "Balloon %d %s, %s: %s" % (r["balloon"], r["characteristic"], TITLES[c], text(r, c))
        if c != "status":
            s += ", status " + r["status"]
        return s

    def description(self):
        return "Row %d of %d, column %d of %d" % (
            self.pos() + 1, len(self.view), self.order.index(self.focus_col) + 1, len(self.order))


SCRIPT = [
    ("Down", ""), ("Right", ""), ("Right", ""), ("Right", ""), ("Right", ""),
    # AC-3: type-to-replace an upper tolerance with "0.0500" and commit.
    ("Type", "0.0500"), ("Enter", ""),
    # Edit again, change, cancel.
    ("F2", ""), ("Backspace", ""), ("Type", "1"), ("Escape", ""),
    # A non-decimal is refused; so is an upper tolerance below the lower one.
    ("Left", ""), ("F2", ""), ("Type", "x"), ("Enter", ""), ("Escape", ""),
    ("Right", ""), ("Type", "-1"), ("Enter", ""), ("Escape", ""),
    # Edit a nominal: deviation and status follow, with the larger number of decimals.
    ("Left", ""), ("Type", "25.1"), ("Enter", ""),
    ("Down", ""), ("Type", "7"), ("Enter", ""), ("Up", ""),
    # Sort by deviation (the focused row stays focused), then navigate.
    ("Right", ""), ("Right", ""), ("Right", ""), ("Right", ""),
    ("SortAsc", ""), ("CtrlHome", ""), ("SortDesc", ""), ("CtrlEnd", ""), ("PageUp", ""),
    ("PageUp", ""),
    # Select 3 rows x 2 columns and copy as TSV.
    ("ShiftDown", ""), ("ShiftDown", ""), ("ShiftRight", ""), ("Copy", ""), ("Escape", ""),
    # Filter by typing, one character per step.
    ("FocusFilter", ""), ("Type", "h"), ("Type", "o"), ("Type", "l"), ("Type", "e"),
    ("Type", " "), ("Type", "1"), ("Backspace", ""), ("Type", "1"), ("Enter", ""),
    ("Down", ""), ("Down", ""), ("Down", ""), ("FlyTo", ""),
    # Reorder and resize columns.
    ("MoveRight", ""), ("MoveLeft", ""), ("MoveLeft", ""), ("Widen", ""), ("Widen", ""),
    ("Narrow", ""), ("Home", ""), ("MoveRight", ""), ("MoveRight", ""), ("Right", ""),
    ("MoveLeft", ""), ("End", ""), ("Right", ""), ("Narrow", ""),
    # Clear the filter and the sort; ends of the table.
    ("FocusFilter", ""), ("Escape", ""), ("SortClear", ""), ("CtrlEnd", ""), ("Down", ""),
    ("PageUp", ""), ("Copy", ""), ("CtrlHome", ""), ("Up", ""), ("Home", ""),
    # Typing or Enter on a non-editable cell does nothing.
    ("Type", "5"), ("Enter", ""), ("F2", ""),
    # Sort and filter that leave nothing shown, then recover.
    ("FocusFilter", ""), ("Type", "zz"), ("Enter", ""), ("Down", ""), ("Copy", ""),
    ("FocusFilter", ""), ("Backspace", ""), ("Backspace", ""), ("Escape", ""),
    ("ShiftRight", ""), ("ShiftDown", ""), ("Copy", ""),
]


def keyboard_text(rows):
    g = Grid([dict(r) for r in rows])
    out = [
        "# fid-spike-ui chartable keyboard v1: step, key, argument, then the state after the step:",
        "# mode, focus row id, focus column, view length, sort, filter, edit buffer, focused cell,",
        "# status, error, anchor, column order, focused column width, clipboard, fly-to,",
        "# header label, cell label, cell description (- for none)",
    ]
    first_copy = None
    for n, (key, arg) in enumerate(SCRIPT, 1):
        g.apply(key, arg)
        if key == "Copy" and first_copy is None:
            first_copy = g.clipboard
        has = g.focus_row is not None
        r = g.row(g.focus_row) if has else None
        clip = "-" if g.clipboard is None else "%d:%016x" % (len(g.clipboard.encode("utf-8")),
                                                             fnv1a(g.clipboard.encode("utf-8")))
        fields = [
            str(n), key, arg, g.mode,
            str(g.focus_row) if has else "-", g.focus_col, str(len(g.view)),
            "-" if g.sort is None else "%s:%s" % g.sort, g.filter,
            g.buf if g.mode == "edit" else "-",
            text(r, g.focus_col) if has else "-", r["status"] if has else "-",
            g.error or "-",
            "-" if g.anchor is None else "%d:%s" % g.anchor,
            ",".join(g.order), str(g.widths[g.focus_col]), clip,
            "-" if g.fly_to is None else str(g.fly_to),
            g.header_label(), g.label() if has else "-", g.description() if has else "-",
        ]
        for f in fields:
            assert "\t" not in f and "\n" not in f, f
        out.append("\t".join(fields))
    assert first_copy is not None
    return "\n".join(out) + "\n", first_copy


# ---------------------------------------------------------------------------------------
# Output files

SORTS = [("deviation", "asc"), ("deviation", "desc"), ("characteristic", "asc"),
         ("status", "desc"), ("actual", "asc"), ("note", "desc"), ("id", "desc")]
FILTERS = [(None, "h"), (None, "ho"), (None, "hol"), (None, "hole"), (None, "hole "),
           (None, "hole 1"), (None, "HOLE 1"), (None, "0.0500"), (None, "not m"), (None, "zz"),
           (("deviation", "desc"), "hole 1"), (("characteristic", "asc"), "datum")]


def ids_line(ids):
    return ",".join(str(i) for i in ids) if ids else "-"


def sort_text(rows):
    out = ["# fid-spike-ui chartable sort v1: column, direction, count, FNV-1a 64 of ids "
           "(one per line), first 20 ids, last 20 ids"]
    for col, d in SORTS:
        ids = view_of(rows, (col, d), "")
        out.append("%s\t%s\t%d\t%016x\t%s\t%s" % (col, d, len(ids), ids_digest(ids),
                                                ids_line(ids[:20]), ids_line(ids[-20:])))
    return "\n".join(out) + "\n"


def filter_text(rows):
    out = ["# fid-spike-ui chartable filter v1: sort (- or column:direction), filter, count, "
           "FNV-1a 64 of ids, first 20 ids"]
    for sort, f in FILTERS:
        ids = view_of(rows, sort, f)
        out.append("%s\t%s\t%d\t%016x\t%s" % ("-" if sort is None else "%s:%s" % sort, f, len(ids),
                                            ids_digest(ids), ids_line(ids[:20])))
    return "\n".join(out) + "\n"


def build():
    rows = generate()
    assert len(rows) == ROWS and len(COLUMNS) == 14
    whole = (HEADER + "".join(tsv_line(r) for r in rows)).encode("utf-8")
    counts = {s: sum(1 for r in rows if r["status"] == s) for s in STATUS_RANK}
    assert all(v > 0 for v in counts.values()), counts
    digest = "rows\t%d\nbytes\t%d\nfnv1a64\t%016x\n" % (ROWS, len(whole), fnv1a(whole))
    picks = sorted(set(range(1, 51)) | set(range(997, ROWS + 1, 997)) | set(range(ROWS - 49, ROWS + 1)))
    sample = HEADER + "".join(tsv_line(rows[i - 1]) for i in picks)
    keyboard, copy = keyboard_text(rows)
    return {
        "digest.txt": digest,
        "sample.tsv": sample,
        "sort.tsv": sort_text(rows),
        "filter.tsv": filter_text(rows),
        "keyboard.tsv": keyboard,
        "copy.tsv": copy,
    }


def main(argv):
    check = "--check" in argv[1:]
    bad = 0
    for name, body in build().items():
        path = os.path.join(HERE, name)
        data = body.encode("utf-8")
        if check:
            try:
                with open(path, "rb") as f:
                    same = f.read() == data
            except OSError:
                same = False
            if not same:
                print("stale: " + path)
                bad += 1
        else:
            with open(path, "wb") as f:
                f.write(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
